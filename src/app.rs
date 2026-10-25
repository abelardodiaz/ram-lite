//! Ventana Win32 + GDI y bandeja.
//!
//! - Ventana emergente sin bordes, fuera de la barra de tareas (WS_EX_TOOLWINDOW), siempre encima
//!   si asi se eligio, esquinas redondeadas de Windows 11 (DWM), se arrastra desde cualquier punto
//!   (HTCAPTION) salvo la X de cerrar, que aparece al pasar el raton y oculta a la bandeja.
//! - DPI por monitor v2: todo se dibuja en px logicos * dpi / 96; WM_DPICHANGED rehace fuentes.
//! - Pintado con doble bufer (bitmap en memoria + BitBlt): sin parpadeo.
//! - Un temporizador cada 2 s lee la memoria en el mismo hilo de la ventana (la llamada no
//!   bloquea) y actualiza la ventana, el historial y el icono de la bandeja.
//! - Preferencias en el registro (`settings`); "Iniciar con Windows" en Run (`autostart`).

use std::cell::RefCell;
use std::ffi::c_void;

use windows::Win32::Foundation::{
    COLORREF, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET, DeleteDC,
    DeleteObject, EndPaint, EnumDisplayMonitors, FF_DONTCARE, FW_BOLD, FW_NORMAL, FillRect,
    GetMonitorInfoW, GetStockObject, GetTextExtentPoint32W, HDC, HFONT, HGDIOBJ, HMONITOR,
    InvalidateRect, LineTo, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MoveToEx, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect,
    SRCCOPY, SelectObject, SetBkMode, SetTextAlign, SetTextCharacterExtra, SetTextColor,
    TA_BASELINE, TA_LEFT, TA_RIGHT, TRANSPARENT, TextOutW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, GetDpiForWindow,
    GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowW, GetClientRect,
    GetCursorPos, GetMessageW, GetWindowRect, HICON, HMENU, HTCAPTION, HTCLIENT, HWND_NOTOPMOST,
    HWND_TOP, HWND_TOPMOST, IDC_ARROW, IsWindowVisible, KillTimer, LoadCursorW, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SM_CXSMICON, SW_HIDE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
    TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCMOUSEMOVE, WM_NCRBUTTONUP, WM_NULL,
    WM_PAINT, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::mem::{self, History, Level, Mem};
use crate::settings::{self, Settings};
use crate::text::{self, Texts};
use crate::{
    AMBER, BG, CLOSE_HOT, DIM, FAINT, GREEN, LINE, MUTED, RED, TEXT, TRACK, autostart, icon,
    placement,
};

const CLASS_NAME: PCWSTR = w!("RamLite");
const MUTEX_NAME: PCWSTR = w!("Local\\io.github.abelardodiaz.ram-lite");

/// Mensajes del icono de la bandeja.
const WM_TRAY: u32 = WM_APP + 1;
/// Otra instancia pide que se muestre esta ventana.
const WM_SHOW_REQ: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const ID_TOGGLE: usize = 1;
const ID_TOPMOST: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_TASKMGR: usize = 4;
const ID_QUIT: usize = 5;
/// Temporizador que vigila si el raton salio de la ventana (para ocultar la X).
const TIMER_HOVER: usize = 1;
/// Temporizador de lectura de la memoria.
const TIMER_SAMPLE: usize = 2;
const SAMPLE_MS: u32 = 2000;

// Medidas en px logicos (a 96 dpi).
const WIDTH: i32 = 260;
const MARGIN_RIGHT: i32 = 24;
const TOP: i32 = 40;
const PAD_X: i32 = 14;
const PAD_TOP: i32 = 11;
const PAD_BOTTOM: i32 = 9;
const TEXT_LINE: i32 = 17;
const BAR_GAP: i32 = 5;
const BAR_H: i32 = 7;
const SPARK_GAP: i32 = 9;
const SPARK_H: i32 = 24;
const FOOT_GAP: i32 = 7;
const FOOT_LINE: i32 = 15;
/// X de cerrar: cuadro de CLOSE_SIZE en la esquina, a CLOSE_INSET del borde derecho y de arriba.
const CLOSE_SIZE: i32 = 13;
const CLOSE_INSET: i32 = 3;

struct Fonts {
    label: HFONT,
    pct: HFONT,
    small: HFONT,
    foot: HFONT,
}

struct Ui {
    texts: &'static Texts,
    dpi: u32,
    fonts: Fonts,
    taskbar_created: u32,
    settings: Settings,
    mem: Option<Mem>,
    history: History,
    /// Icono actual de la bandeja y el estado con el que se dibujo (porcentaje entero y nivel).
    tray_icon: HICON,
    tray_state: Option<(u32, Level)>,
    tray_size: i32,
    /// El raton esta sobre la ventana: se dibuja la X.
    hover: bool,
    /// El raton esta sobre la X.
    hot: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// COLORREF de GDI (0x00BBGGRR) a partir de 0xRRGGBB.
fn cref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff))
}

fn level_rgb(level: Level) -> u32 {
    match level {
        Level::Normal => GREEN,
        Level::High => AMBER,
        Level::Critical => RED,
    }
}

/// px logicos -> px fisicos para un dpi.
fn px(v: i32, dpi: u32) -> i32 {
    ((v as i64 * dpi as i64 + 48) / 96) as i32
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Coordenadas con signo de un LPARAM de raton.
fn point_of(lparam: LPARAM) -> (i32, i32) {
    let v = lparam.0 as usize;
    (
        (v & 0xffff) as u16 as i16 as i32,
        ((v >> 16) & 0xffff) as u16 as i16 as i32,
    )
}

fn window_height(dpi: u32) -> i32 {
    let row = TEXT_LINE + BAR_GAP + BAR_H;
    px(
        PAD_TOP + row + SPARK_GAP + SPARK_H + FOOT_GAP + FOOT_LINE + PAD_BOTTOM,
        dpi,
    )
}

/// Cuadro de la X en coordenadas de cliente, para un ancho de ventana `width`.
fn close_rect(width: i32, dpi: u32) -> RECT {
    let right = width - px(CLOSE_INSET, dpi);
    let top = px(CLOSE_INSET, dpi) - px(1, dpi);
    RECT {
        left: right - px(CLOSE_SIZE, dpi),
        top,
        right,
        bottom: top + px(CLOSE_SIZE, dpi),
    }
}

fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn make_font(size_tenths: i32, bold: bool, dpi: u32) -> HFONT {
    // Alto negativo = tamano de la letra en px (igual que font-size de CSS).
    let height = -((size_tenths as i64 * dpi as i64 + 480) / 960) as i32;
    let weight = if bold { FW_BOLD.0 } else { FW_NORMAL.0 } as i32;
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
    }
}

fn make_fonts(dpi: u32) -> Fonts {
    Fonts {
        label: make_font(120, true, dpi),
        pct: make_font(140, true, dpi),
        small: make_font(115, false, dpi),
        foot: make_font(110, false, dpi),
    }
}

fn free_fonts(f: &Fonts) {
    unsafe {
        for font in [f.label, f.pct, f.small, f.foot] {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

/// "RAM 68% - 10.9 / 15.9 GB" para la bandeja.
fn tooltip(m: Option<&Mem>) -> String {
    match m {
        Some(m) => format!(
            "RAM {} - {} / {} GB",
            mem::percent_text(m.percent()),
            mem::gb(m.used()),
            mem::gb(m.total)
        ),
        None => "RAM".to_string(),
    }
}

fn tray_data(hwnd: HWND, icon: HICON, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *dst = src;
    }
    nid
}

fn add_tray(hwnd: HWND, icon: HICON, tip: &str) {
    let nid = tray_data(hwnd, icon, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn toggle(hwnd: HWND, topmost: bool) {
    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        } else {
            show_requested(hwnd, topmost);
        }
    }
}

fn apply_topmost(hwnd: HWND, topmost: bool) {
    let after = if topmost {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Muestra la ventana sin activarla (arranque: no roba el foco al iniciar sesion).
fn show(hwnd: HWND, topmost: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    apply_topmost(hwnd, topmost);
}

/// Muestra la ventana porque el usuario la pidio (bandeja o segunda instancia).
fn show_requested(hwnd: HWND, topmost: bool) {
    show(hwnd, topmost);
    if !topmost {
        // Sin "Siempre encima" la ventana puede quedar detras de otras: se trae al frente.
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn show_menu(hwnd: HWND, t: &Texts, topmost: bool) {
    let toggle_text = wide_z(t.toggle);
    let topmost_text = wide_z(t.topmost);
    let autostart_text = wide_z(t.autostart);
    let taskmgr_text = wide_z(t.taskmgr);
    let quit_text = wide_z(t.quit);
    let checked = |on: bool| {
        if on {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        }
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_TOGGLE, PCWSTR(toggle_text.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, ID_TASKMGR, PCWSTR(taskmgr_text.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            checked(topmost),
            ID_TOPMOST,
            PCWSTR(topmost_text.as_ptr()),
        );
        // La marca sale del registro cada vez que se abre el menu.
        let _ = AppendMenuW(
            menu,
            checked(autostart::is_enabled()),
            ID_AUTOSTART,
            PCWSTR(autostart_text.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, PCWSTR(quit_text.as_ptr()));
        popup(hwnd, menu);
    }
}

unsafe fn popup(hwnd: HWND, menu: HMENU) {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Sin esto el menu no se cierra al hacer clic fuera (KB135788).
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

fn open_task_manager() {
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            w!("taskmgr.exe"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn fill(hdc: HDC, rect: &RECT, rgb: u32) {
    unsafe {
        let brush = CreateSolidBrush(cref(rgb));
        FillRect(hdc, rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// Rectangulo redondeado relleno, sin borde.
fn round_fill(hdc: HDC, r: &RECT, radius: i32, rgb: u32) {
    unsafe {
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let brush = CreateSolidBrush(cref(rgb));
        let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
        // Con lapiz nulo RoundRect no pinta la ultima fila/columna: +1 para el tamano exacto.
        let _ = RoundRect(
            hdc,
            r.left,
            r.top,
            r.right + 1,
            r.bottom + 1,
            radius,
            radius,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// Barra redondeada (pista + relleno). `frac` en [0, 1].
fn bar(hdc: HDC, x: i32, y: i32, w: i32, h: i32, frac: f64, rgb: u32) {
    round_fill(
        hdc,
        &RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        h,
        TRACK,
    );
    if frac > 0.0 {
        let fw = ((w as f64 * frac).round() as i32).max(h);
        round_fill(
            hdc,
            &RECT {
                left: x,
                top: y,
                right: x + fw,
                bottom: y + h,
            },
            h,
            rgb,
        );
    }
}

fn text_width(hdc: HDC, font: HFONT, s: &[u16]) -> i32 {
    unsafe {
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, s, &mut size);
        SelectObject(hdc, old);
        size.cx
    }
}

/// Fila principal: "RAM  68%" a la izquierda, "10.9 / 15.9 GB" a la derecha y la barra debajo.
fn ram_row(hdc: HDC, ui: &Ui, top: i32, width: i32) {
    let d = ui.dpi;
    let x0 = px(PAD_X, d);
    let x1 = width - px(PAD_X, d);
    let baseline = top + px(13, d);
    let pct = ui.mem.map(|m| m.percent());
    unsafe {
        SetTextAlign(hdc, TA_LEFT | TA_BASELINE);
        SetTextColor(hdc, cref(TEXT));
        let label = wide("RAM");
        SelectObject(hdc, HGDIOBJ(ui.fonts.label.0));
        // letter-spacing .05em de la UI de claude-usage-widgets.
        SetTextCharacterExtra(hdc, px(1, d).min(((12 * d) as i32 + 960) / 1920).max(0));
        let _ = TextOutW(hdc, x0, baseline, &label);
        let lw = text_width(hdc, ui.fonts.label, &label);
        SetTextCharacterExtra(hdc, 0);
        let pct_text = wide(&pct.map_or("--".to_string(), mem::percent_text));
        SelectObject(hdc, HGDIOBJ(ui.fonts.pct.0));
        let _ = TextOutW(hdc, x0 + lw + px(6, d), baseline, &pct_text);
        if let Some(m) = ui.mem {
            let right = format!("{} / {} GB", mem::gb(m.used()), mem::gb(m.total));
            SetTextAlign(hdc, TA_RIGHT | TA_BASELINE);
            SetTextColor(hdc, cref(MUTED));
            SelectObject(hdc, HGDIOBJ(ui.fonts.small.0));
            let _ = TextOutW(hdc, x1, baseline, &wide(&right));
        }
    }
    let (frac, rgb) = pct.map_or((0.0, TRACK), |p| (p / 100.0, level_rgb(mem::level(p))));
    bar(
        hdc,
        x0,
        top + px(TEXT_LINE + BAR_GAP, d),
        x1 - x0,
        px(BAR_H, d),
        frac,
        rgb,
    );
}

/// Ultimos 2 minutos: una columna por muestra, llenando desde la derecha. Gris si estaba en nivel
/// normal; ambar o rojo si estaba alta.
fn sparkline(hdc: HDC, ui: &Ui, top: i32, width: i32) {
    let d = ui.dpi;
    let x0 = px(PAD_X, d);
    let x1 = width - px(PAD_X, d);
    let h = px(SPARK_H, d);
    let area = RECT {
        left: x0,
        top,
        right: x1,
        bottom: top + h,
    };
    round_fill(hdc, &area, px(6, d), TRACK);
    let n = mem::HISTORY as f64;
    let slot = (x1 - x0) as f64 / n;
    let first = mem::HISTORY - ui.history.len();
    let gap = if slot >= 3.0 { 1.0 } else { 0.0 };
    for (i, p) in ui.history.iter().enumerate() {
        let left = x0 as f64 + (first + i) as f64 * slot;
        let col_h = ((h as f64 * p / 100.0).round() as i32).clamp(1, h);
        let rgb = match mem::level(p) {
            Level::Normal => DIM,
            lvl => level_rgb(lvl),
        };
        fill(
            hdc,
            &RECT {
                left: left.round() as i32,
                top: top + h - col_h,
                right: (left + slot - gap).round() as i32,
                bottom: top + h,
            },
            rgb,
        );
    }
}

/// Pie: "Libre 5.0 GB" a la izquierda, "Confirmada 18.2 / 24.0 GB" a la derecha.
fn footer(hdc: HDC, ui: &Ui, top: i32, width: i32) {
    let Some(m) = ui.mem else {
        return;
    };
    let d = ui.dpi;
    let baseline = top + px(11, d);
    let left = format!("{} {} GB", ui.texts.free, mem::gb(m.avail));
    let right = format!(
        "{} {} / {} GB",
        ui.texts.commit,
        mem::gb(m.commit_used),
        mem::gb(m.commit_limit)
    );
    unsafe {
        SelectObject(hdc, HGDIOBJ(ui.fonts.foot.0));
        SetTextColor(hdc, cref(FAINT));
        SetTextAlign(hdc, TA_LEFT | TA_BASELINE);
        let _ = TextOutW(hdc, px(PAD_X, d), baseline, &wide(&left));
        SetTextAlign(hdc, TA_RIGHT | TA_BASELINE);
        let _ = TextOutW(hdc, width - px(PAD_X, d), baseline, &wide(&right));
    }
}

/// X de cerrar (solo con el raton encima de la ventana).
fn close_button(hdc: HDC, ui: &Ui, width: i32) {
    let d = ui.dpi;
    let r = close_rect(width, d);
    if ui.hot {
        round_fill(hdc, &r, px(4, d), CLOSE_HOT);
    }
    let inset = px(4, d);
    let pen_w = px(1, d).max(1) + i32::from(d >= 144);
    unsafe {
        let pen = CreatePen(PS_SOLID, pen_w, cref(if ui.hot { TEXT } else { MUTED }));
        let old = SelectObject(hdc, HGDIOBJ(pen.0));
        let (l, t) = (r.left + inset, r.top + inset);
        let (rr, b) = (r.right - inset, r.bottom - inset);
        let _ = MoveToEx(hdc, l, t, None);
        let _ = LineTo(hdc, rr, b);
        let _ = MoveToEx(hdc, rr - 1, t, None);
        let _ = LineTo(hdc, l - 1, b);
        SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

fn paint(hwnd: HWND, ui: &Ui) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let mem_dc = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old_bmp = SelectObject(mem_dc, HGDIOBJ(bmp.0));
        let old_font = SelectObject(mem_dc, HGDIOBJ(ui.fonts.foot.0));
        SetBkMode(mem_dc, TRANSPARENT);
        fill(mem_dc, &rc, BG);

        let d = ui.dpi;
        let row_top = px(PAD_TOP, d);
        let spark_top = row_top + px(TEXT_LINE + BAR_GAP + BAR_H + SPARK_GAP, d);
        let foot_top = spark_top + px(SPARK_H + FOOT_GAP, d);
        ram_row(mem_dc, ui, row_top, w);
        sparkline(mem_dc, ui, spark_top, w);
        footer(mem_dc, ui, foot_top, w);
        if ui.hover {
            close_button(mem_dc, ui, w);
        }

        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem_dc), 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_font);
        SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn loword(v: usize) -> u32 {
    (v & 0xffff) as u32
}

/// Acceso corto al estado de la ventana. Nunca se llama a Win32 que despache mensajes con el
/// prestamo activo (TrackPopupMenu, SetWindowPos, DestroyWindow reentran en `wndproc`).
fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| {
        cell.try_borrow_mut()
            .ok()
            .and_then(|mut slot| slot.as_mut().map(f))
    })
}

fn redraw(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Lee la memoria y actualiza historial, ventana e icono de la bandeja. El icono solo se
/// redibuja cuando cambia el porcentaje entero o el nivel; el texto de ayuda, siempre.
fn sample(hwnd: HWND) {
    let Some(m) = mem::read() else {
        return;
    };
    let p = m.percent();
    let state = (p.round() as u32, mem::level(p));
    let tray = with_ui(|ui| {
        ui.mem = Some(m);
        ui.history.push(p);
        let new_icon = if ui.tray_state != Some(state) {
            ui.tray_state = Some(state);
            let fresh = icon::create(ui.tray_size, p / 100.0, level_rgb(state.1));
            let old = std::mem::replace(&mut ui.tray_icon, fresh);
            Some(old)
        } else {
            None
        };
        (ui.tray_icon, new_icon)
    });
    if let Some((current, replaced)) = tray {
        let nid = tray_data(hwnd, current, &tooltip(Some(&m)));
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
            if let Some(old) = replaced.filter(|i| !i.is_invalid()) {
                let _ = DestroyIcon(old);
            }
        }
    }
    if unsafe { IsWindowVisible(hwnd).as_bool() } {
        redraw(hwnd);
    }
}

/// El raton se movio sobre la ventana (`on_close`: sobre la X). Arranca el temporizador que
/// detecta cuando sale.
fn mouse_over(hwnd: HWND, on_close: bool) {
    let changed = with_ui(|ui| {
        let changed = !ui.hover || ui.hot != on_close;
        ui.hover = true;
        ui.hot = on_close;
        changed
    })
    .unwrap_or(false);
    unsafe {
        SetTimer(Some(hwnd), TIMER_HOVER, 100, None);
    }
    if changed {
        redraw(hwnd);
    }
}

/// Temporizador: si el raton ya no esta sobre la ventana, se quita la X.
fn hover_check(hwnd: HWND) {
    let mut pt = POINT::default();
    let mut wr = RECT::default();
    let inside = unsafe {
        GetCursorPos(&mut pt).is_ok()
            && GetWindowRect(hwnd, &mut wr).is_ok()
            && IsWindowVisible(hwnd).as_bool()
            && in_rect(&wr, pt.x, pt.y)
    };
    if inside {
        return;
    }
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_HOVER);
    }
    let changed = with_ui(|ui| {
        let changed = ui.hover || ui.hot;
        ui.hover = false;
        ui.hot = false;
        changed
    })
    .unwrap_or(false);
    if changed {
        redraw(hwnd);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some((texts, taskbar_created, topmost, dpi, tray_icon, current_mem)) = with_ui(|ui| {
        (
            ui.texts,
            ui.taskbar_created,
            ui.settings.topmost,
            ui.dpi,
            ui.tray_icon,
            ui.mem,
        )
    }) else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    match msg {
        WM_NCHITTEST => {
            // Toda la ventana arrastra (HTCAPTION) salvo la X, que es "cliente" para recibir el clic.
            let (sx, sy) = point_of(lparam);
            let mut wr = RECT::default();
            let on_close = unsafe { GetWindowRect(hwnd, &mut wr).is_ok() }
                && in_rect(
                    &close_rect(wr.right - wr.left, dpi),
                    sx - wr.left,
                    sy - wr.top,
                );
            LRESULT(if on_close { HTCLIENT } else { HTCAPTION } as isize)
        }
        // Doble clic en la "barra de titulo" no maximiza ni hace nada.
        WM_NCLBUTTONDBLCLK => LRESULT(0),
        WM_NCRBUTTONUP | WM_RBUTTONUP => {
            show_menu(hwnd, texts, topmost);
            LRESULT(0)
        }
        WM_NCMOUSEMOVE => {
            mouse_over(hwnd, false);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            // Solo llega sobre la X (lo demas es HTCAPTION).
            mouse_over(hwnd, true);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (cx, cy) = point_of(lparam);
            let mut rc = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rc);
            }
            if in_rect(&close_rect(rc.right - rc.left, dpi), cx, cy) {
                // Cerrar = ocultar a la bandeja; se sale desde el menu.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                hover_check(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_HOVER => {
            hover_check(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_SAMPLE => {
            sample(hwnd);
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            // Termino un arrastre: se guarda la posicion.
            let mut wr = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut wr).is_ok() } {
                let pos = [wr.left, wr.top];
                let changed = with_ui(|ui| {
                    let changed = ui.settings.pos != Some(pos);
                    ui.settings.pos = Some(pos);
                    changed
                })
                .unwrap_or(false);
                if changed {
                    settings::save_pos(pos);
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            // Se pinta con el prestamo de `UI` activo: GDI y BitBlt no despachan mensajes.
            if with_ui(|ui| paint(hwnd, ui)).is_none() {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            LRESULT(0)
        }
        WM_SHOW_REQ => {
            show_requested(hwnd, topmost);
            LRESULT(0)
        }
        WM_TRAY => {
            match loword(lparam.0 as usize) {
                WM_LBUTTONUP => toggle(hwnd, topmost),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd, texts, topmost),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match loword(wparam.0) as usize {
                ID_TOGGLE => toggle(hwnd, topmost),
                ID_TASKMGR => open_task_manager(),
                ID_TOPMOST => {
                    let on = !topmost;
                    with_ui(|ui| ui.settings.topmost = on);
                    settings::save_topmost(on);
                    apply_topmost(hwnd, on);
                }
                ID_AUTOSTART => {
                    autostart::set_enabled(!autostart::is_enabled());
                }
                ID_QUIT => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = ((wparam.0 >> 16) & 0xffff) as u32;
            with_ui(|ui| {
                free_fonts(&ui.fonts);
                ui.dpi = dpi;
                ui.fonts = make_fonts(dpi);
            });
            // Se respeta la esquina sugerida por Windows; el tamano sale de nuestras medidas.
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    px(WIDTH, dpi),
                    window_height(dpi),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            redraw(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let nid = tray_data(hwnd, tray_icon, "");
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
                let _ = KillTimer(Some(hwnd), TIMER_SAMPLE);
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        m if m != 0 && m == taskbar_created => {
            // Explorer se reinicio: hay que volver a poner el icono.
            add_tray(hwnd, tray_icon, &tooltip(current_mem.as_ref()));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn collect_monitor(
    _monitor: HMONITOR,
    _hdc: HDC,
    rect: *mut RECT,
    data: LPARAM,
) -> windows::core::BOOL {
    unsafe {
        let list = &mut *(data.0 as *mut Vec<placement::Rect>);
        if let Some(r) = rect.as_ref() {
            list.push([r.left, r.top, r.right, r.bottom]);
        }
    }
    windows::core::BOOL(1)
}

/// Rectangulos de todos los monitores conectados, en px fisicos de pantalla virtual.
fn monitors() -> Vec<placement::Rect> {
    let mut list: Vec<placement::Rect> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut list as *mut _ as isize),
        );
    }
    list
}

fn monitor_dpi(monitor: HMONITOR) -> u32 {
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    }
    dpi_x.max(96)
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Una sola instancia: si ya hay otra, se le pide que se muestre y se sale.
        let _mutex = CreateMutexW(None, false, MUTEX_NAME);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(other) = FindWindowW(CLASS_NAME, PCWSTR::null()) {
                let _ = PostMessageW(Some(other), WM_SHOW_REQ, WPARAM(0), LPARAM(0));
            }
            return;
        }

        let prefs = settings::load();
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let instance = module.into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return;
        }

        // De fabrica: arriba a la derecha del monitor principal. Si hay posicion guardada y
        // sigue visible, esa.
        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(primary, &mut mi);
        let pdpi = monitor_dpi(primary);
        let m = mi.rcMonitor;
        let fallback = placement::default_pos(
            [m.left, m.top, m.right, m.bottom],
            px(WIDTH, pdpi),
            px(MARGIN_RIGHT, pdpi),
            px(TOP, pdpi),
        );
        let [left, top] = placement::choose(
            prefs.pos,
            (px(WIDTH, pdpi), window_height(pdpi)),
            &monitors(),
            fallback,
        );
        let dpi = monitor_dpi(MonitorFromPoint(
            POINT { x: left, y: top },
            MONITOR_DEFAULTTONEAREST,
        ));

        let ex_style = if prefs.topmost {
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST
        } else {
            WS_EX_TOOLWINDOW
        };
        let Ok(hwnd) = CreateWindowExW(
            ex_style,
            CLASS_NAME,
            w!("RAM"),
            WS_POPUP,
            left,
            top,
            px(WIDTH, dpi),
            window_height(dpi),
            None,
            None,
            Some(instance),
            None,
        ) else {
            return;
        };

        let corner: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        // Borde fino del color --line (sin esto Windows 11 lo pinta gris claro).
        let border = cref(LINE);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border as *const _ as *const c_void,
            std::mem::size_of::<COLORREF>() as u32,
        );

        let real_dpi = GetDpiForWindow(hwnd).max(96);
        let tray_size = GetSystemMetricsForDpi(SM_CXSMICON, real_dpi).max(16);
        let first = mem::read();
        let mut history = History::new();
        let (tray_icon, tray_state) = match first {
            Some(m) => {
                let p = m.percent();
                history.push(p);
                let lvl = mem::level(p);
                (
                    icon::create(tray_size, p / 100.0, level_rgb(lvl)),
                    Some((p.round() as u32, lvl)),
                )
            }
            None => (icon::create(tray_size, 0.0, GREEN), None),
        };
        let topmost = prefs.topmost;
        let ui = Ui {
            texts: text::current(),
            dpi: real_dpi,
            fonts: make_fonts(real_dpi),
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            settings: prefs,
            mem: first,
            history,
            tray_icon,
            tray_state,
            tray_size,
            hover: false,
            hot: false,
        };
        if real_dpi != dpi {
            let _ = SetWindowPos(
                hwnd,
                None,
                left,
                top,
                px(WIDTH, real_dpi),
                window_height(real_dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        add_tray(hwnd, tray_icon, &tooltip(first.as_ref()));
        UI.with_borrow_mut(|slot| *slot = Some(ui));

        SetTimer(Some(hwnd), TIMER_SAMPLE, SAMPLE_MS, None);
        show(hwnd, topmost);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UI.with_borrow_mut(|slot| {
            if let Some(ui) = slot.take() {
                free_fonts(&ui.fonts);
                if !ui.tray_icon.is_invalid() {
                    let _ = DestroyIcon(ui.tray_icon);
                }
            }
        });
    }
}
