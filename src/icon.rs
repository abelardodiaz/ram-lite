//! Icono de la bandeja dibujado en tiempo de ejecucion: un cuadro oscuro con un medidor vertical
//! que se llena segun la RAM en uso, del color del nivel. Asi el porcentaje se ve sin abrir nada
//! y el exe no necesita archivo .ico ni recursos.

use std::ffi::c_void;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

use crate::{BG, DIM, TRACK};

/// Mezcla de dos colores 0xRRGGBB; `t` = peso de `b`.
fn mix(a: u32, b: u32, t: f64) -> u32 {
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f64;
    let m = |s: u32| ((ch(a, s) * (1.0 - t) + ch(b, s) * t).round() as u32) << s;
    m(16) | m(8) | m(0)
}

/// Pixeles 0xAARRGGBB premultiplicados (BGRA en memoria), fila 0 arriba. `frac` en [0, 1].
pub fn pixels(size: u32, frac: f64, fill: u32) -> Vec<u32> {
    let s = size as f64;
    let half = s / 2.0;
    let radius = (s * 0.22).max(2.0);
    let inset = (size / 8).max(2) as f64 + 1.0;
    let inner_top = inset;
    let inner_bottom = s - inset;
    let fill_h = ((inner_bottom - inner_top) * frac.clamp(0.0, 1.0)).round();
    let fill_h = if frac > 0.0 { fill_h.max(1.0) } else { 0.0 };
    let fill_top = inner_bottom - fill_h;
    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            // Distancia con signo al borde del cuadro redondeado (negativa = dentro).
            let qx = (px - half).abs() - (half - radius);
            let qy = (py - half).abs() - (half - radius);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            let d = outside + qx.max(qy).min(0.0) - radius;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                out.push(0);
                continue;
            }
            let in_gauge = px > inset && px < s - inset && py > inner_top && py < inner_bottom;
            let rgb = if d > -1.0 {
                // Borde de 1 px: que se distinga sobre una barra de tareas oscura.
                mix(BG, DIM, (d + 1.0).clamp(0.0, 1.0).max(0.6))
            } else if in_gauge {
                if py >= fill_top { fill } else { TRACK }
            } else {
                BG
            };
            let a = (cover * 255.0).round() as u32;
            let pre = |sh: u32| ((((rgb >> sh) & 0xff) * a + 127) / 255) << sh;
            out.push((a << 24) | pre(16) | pre(8) | pre(0));
        }
    }
    out
}

/// Crea el HICON. Quien lo recibe lo libera con DestroyIcon.
pub fn create(size: i32, frac: f64, fill: u32) -> HICON {
    let size = size.clamp(16, 256);
    let px = pixels(size as u32, frac, fill);
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                // Alto negativo: filas de arriba hacia abajo, igual que `pixels`.
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return HICON::default();
        };
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(color.0));
            return HICON::default();
        }
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, px.len());
        // Mascara monocroma en ceros: con 32 bits la transparencia sale del canal alfa.
        let stride = (size as usize).div_ceil(16) * 2;
        let zeros = vec![0u8; stride * size as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(zeros.as_ptr() as *const c_void));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        // CreateIconIndirect copia los bitmaps: se liberan aqui.
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILL: u32 = 0x3fbf6f;

    fn at(px: &[u32], size: u32, x: u32, y: u32) -> u32 {
        px[(y * size + x) as usize]
    }

    #[test]
    fn tamano_y_esquinas_transparentes() {
        for size in [16u32, 20, 24, 32] {
            let px = pixels(size, 0.5, FILL);
            assert_eq!(px.len(), (size * size) as usize);
            assert_eq!(at(&px, size, 0, 0), 0, "esquina a {size}px");
            assert_eq!(at(&px, size, size - 1, size - 1), 0);
        }
    }

    #[test]
    fn lleno_abajo_vacio_arriba() {
        let size = 32;
        let px = pixels(size, 0.5, FILL);
        let mid = size / 2;
        assert_eq!(at(&px, size, mid, size - 7), 0xff00_0000 | FILL);
        assert_eq!(at(&px, size, mid, 7), 0xff00_0000 | TRACK);
    }

    #[test]
    fn cero_sin_relleno_y_uno_lleno() {
        let size = 16;
        let mid = size / 2;
        let empty = pixels(size, 0.0, FILL);
        assert!(empty.iter().all(|&p| p & 0x00ff_ffff != FILL));
        let full = pixels(size, 1.0, FILL);
        assert_eq!(at(&full, size, mid, 4), 0xff00_0000 | FILL);
        assert_eq!(at(&full, size, mid, size - 5), 0xff00_0000 | FILL);
    }

    #[test]
    fn crea_un_hicon_real() {
        use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
        for size in [16, 24, 32] {
            let icon = create(size, 0.65, FILL);
            assert!(!icon.is_invalid(), "HICON nulo a {size}px");
            unsafe { DestroyIcon(icon) }.expect("DestroyIcon");
        }
    }

    #[test]
    fn premultiplicado_valido() {
        for p in pixels(20, 0.7, FILL) {
            let a = p >> 24;
            for sh in [16, 8, 0] {
                assert!((p >> sh) & 0xff <= a, "canal mayor que alfa: {p:08x}");
            }
        }
    }
}
