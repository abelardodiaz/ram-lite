//! "Iniciar con Windows": valor `ram-lite` en
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Run apuntando, entre comillas, al exe que esta
//! corriendo (`std::env::current_exe`). Asi funciona igual instalado con `instalar.ps1` (ruta
//! fija en %LOCALAPPDATA%) que descargado de Releases en cualquier carpeta. El registro es la
//! unica verdad: la marca del menu se lee de ahi cada vez que se abre, y solo sale marcada si el
//! valor apunta a ESTE exe (si apunta a otro, al activarla se reescribe con el actual).

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("ram-lite");

/// Comando que se guarda en Run: la ruta entre comillas (admite espacios).
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// El comando guardado en Run arranca `exe`. Tolera comillas o no, espacios alrededor y
/// mayusculas distintas (las rutas de Windows no distinguen mayusculas), y `/` frente a `\`.
pub fn points_to(stored: &str, exe: &Path) -> bool {
    fn norm(s: &str) -> String {
        s.trim()
            .trim_matches('"')
            .trim()
            .replace('/', "\\")
            .to_lowercase()
    }
    let stored = norm(stored);
    !stored.is_empty() && stored == norm(&exe.display().to_string())
}

/// Valor guardado en Run, si existe.
fn stored_value() -> Option<String> {
    let mut size = 0u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if rc != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Run tiene un valor que arranca este mismo exe.
pub fn is_enabled() -> bool {
    match (stored_value(), std::env::current_exe()) {
        (Some(stored), Ok(exe)) => points_to(&stored, &exe),
        _ => false,
    }
}

/// Crea (con el exe actual) o borra el valor. Devuelve el estado real tras el cambio.
pub fn set_enabled(on: bool) -> bool {
    unsafe {
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let cmd = run_command(&exe);
                let data: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    RUN_KEY,
                    VALUE,
                    REG_SZ.0,
                    Some(data.as_ptr() as *const c_void),
                    (data.len() * 2) as u32,
                );
            }
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE);
        }
    }
    is_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Users\Ana Maria\Downloads\ram-lite_0.1.0_x64.exe")
    }

    #[test]
    fn comando_entre_comillas_y_reconocido() {
        let cmd = run_command(&exe());
        assert!(cmd.starts_with('"') && cmd.ends_with('"'));
        assert!(points_to(&cmd, &exe()));
    }

    #[test]
    fn tolera_sin_comillas_mayusculas_y_barras() {
        let p = exe();
        assert!(points_to(&p.display().to_string(), &p));
        assert!(points_to(
            "  \"c:/users/ana maria/downloads/RAM-LITE_0.1.0_X64.EXE\"  ",
            &p
        ));
    }

    #[test]
    fn otra_ruta_no_cuenta() {
        let fixed = r#""C:\Users\Ana Maria\AppData\Local\ram-lite\ram-lite.exe""#;
        assert!(!points_to(fixed, &exe()));
        assert!(!points_to("", &exe()));
        assert!(!points_to("\"\"", &exe()));
    }
}
