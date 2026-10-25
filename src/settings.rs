//! Preferencias en el registro, HKCU\Software\ram-lite: siempre encima y posicion de la ventana.
//! Sin archivos ni JSON. Un valor que falte o no se pueda leer cae a lo de fabrica.

use std::ffi::c_void;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

const KEY: PCWSTR = w!("Software\\ram-lite");
const TOPMOST: PCWSTR = w!("Topmost");
const POS_X: PCWSTR = w!("PosX");
const POS_Y: PCWSTR = w!("PosY");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub topmost: bool,
    /// Esquina superior izquierda en px fisicos de pantalla.
    pub pos: Option<[i32; 2]>,
}

fn get(name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    (rc == ERROR_SUCCESS).then_some(value)
}

fn set(name: PCWSTR, value: u32) {
    // RegSetKeyValueW crea la clave si aun no existe.
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            REG_DWORD.0,
            Some(&value as *const u32 as *const c_void),
            std::mem::size_of::<u32>() as u32,
        );
    }
}

pub fn load() -> Settings {
    let pos = match (get(POS_X), get(POS_Y)) {
        // Las coordenadas pueden ser negativas (monitor a la izquierda): DWORD <-> i32 tal cual.
        (Some(x), Some(y)) => Some([x as i32, y as i32]),
        _ => None,
    };
    Settings {
        topmost: get(TOPMOST).is_none_or(|v| v != 0),
        pos,
    }
}

pub fn save_topmost(on: bool) {
    set(TOPMOST, u32::from(on));
}

pub fn save_pos(pos: [i32; 2]) {
    set(POS_X, pos[0] as u32);
    set(POS_Y, pos[1] as u32);
}
