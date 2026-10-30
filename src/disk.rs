//! Espacio de los discos (GetDiskFreeSpaceExW) y de las distros de WSL. Solo se lee lo que el
//! usuario marco en el menu, cada 30 s. WSL se mide solo si ya esta corriendo (proceso vmmemWSL),
//! para no despertarlo; el peso del ext4.vhdx se lee siempre, porque es un archivo de Windows.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetFileAttributesExW, GetFileExInfoStandard,
    GetLogicalDrives, GetVolumeInformationW, WIN32_FILE_ATTRIBUTE_DATA,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::core::{PCWSTR, PWSTR, w};

/// GetDriveTypeW: disco extraible (USB, memoria) y disco fijo.
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

const LXSS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Lxss";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Space {
    pub total: u64,
    pub free: u64,
}

impl Space {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    /// Porcentaje ocupado, en [0, 100].
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.used() as f64 * 100.0 / self.total as f64).clamp(0.0, 100.0)
    }
}

/// Una distro de WSL registrada.
#[derive(Debug, Clone, PartialEq)]
pub struct Distro {
    pub name: String,
    /// Ruta del disco virtual en Windows (BasePath\ext4.vhdx).
    pub vhdx: String,
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Espacio total y libre de una ruta raiz ("C:\", "\\wsl.localhost\Ubuntu\").
pub fn space(root: &str) -> Option<Space> {
    let path = wide_z(root);
    let (mut total, mut free) = (0u64, 0u64);
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(path.as_ptr()),
            None,
            Some(&mut total),
            Some(&mut free),
        )
    }
    .ok()?;
    Some(Space { total, free })
}

/// Letras presentes en una mascara de GetLogicalDrives (bit 0 = A).
pub fn letters(mask: u32) -> impl Iterator<Item = char> {
    (0..26u8)
        .filter(move |i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i) as char)
}

/// Bit de una letra en las mascaras de unidades.
pub fn bit(letter: char) -> u32 {
    let i = (letter.to_ascii_uppercase() as u32).wrapping_sub('A' as u32);
    if i < 26 { 1 << i } else { 0 }
}

pub fn root(letter: char) -> String {
    format!("{letter}:\\")
}

/// Unidades fijas o extraibles conectadas ahora (sin red ni CD).
pub fn drives() -> Vec<char> {
    let mask = unsafe { GetLogicalDrives() };
    letters(mask)
        .filter(|&l| {
            let path = wide_z(&root(l));
            let kind = unsafe { GetDriveTypeW(PCWSTR(path.as_ptr())) };
            kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE
        })
        .collect()
}

/// Etiqueta del volumen ("Ventoy", "VTOYEFI"); `None` si no tiene o no se puede leer.
pub fn label(letter: char) -> Option<String> {
    let path = wide_z(&root(letter));
    let mut buf = [0u16; 261];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(path.as_ptr()),
            Some(&mut buf),
            None,
            None,
            None,
            None,
        )
    }
    .ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
}

/// "212.8 / 237.3 GB"; si el disco mide menos de 1 GB, en MB ("27.1 / 31.7 MB") para que una
/// particion chica no salga como "0.0 / 0.0 GB".
pub fn space_text(s: &Space) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let (unit, div) = if (s.total as f64) < GIB {
        ("MB", 1024.0 * 1024.0)
    } else {
        ("GB", GIB)
    };
    format!(
        "{:.1} / {:.1} {unit}",
        s.used() as f64 / div,
        s.total as f64 / div
    )
}

/// Tamano corto para el menu: "116 GB", "32 MB".
pub fn size_short(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{} GB", (bytes + GIB / 2) / GIB)
    } else {
        format!("{} MB", (bytes + MIB / 2) / MIB)
    }
}

/// Tamano de un archivo en bytes.
pub fn file_size(path: &str) -> Option<u64> {
    let p = wide_z(path);
    let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
    unsafe {
        GetFileAttributesExW(
            PCWSTR(p.as_ptr()),
            GetFileExInfoStandard,
            &mut data as *mut _ as *mut c_void,
        )
    }
    .ok()?;
    Some((u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow))
}

fn reg_string(subkey: &str, name: PCWSTR) -> Option<String> {
    let key = wide_z(subkey);
    let mut buf = [0u16; 520];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            name,
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

/// Distros de WSL registradas para este usuario (HKCU\...\Lxss), sin tocar WSL.
pub fn distros() -> Vec<Distro> {
    let mut out = Vec::new();
    let lxss = wide_z(LXSS);
    let mut key = HKEY::default();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(lxss.as_ptr()),
            Some(0),
            KEY_READ,
            &mut key,
        )
    };
    if rc != ERROR_SUCCESS {
        return out;
    }
    for index in 0.. {
        let mut buf = [0u16; 64];
        let mut len = buf.len() as u32;
        let rc = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if rc != ERROR_SUCCESS {
            break;
        }
        let guid = String::from_utf16_lossy(&buf[..len as usize]);
        let sub = format!("{LXSS}\\{guid}");
        if let (Some(name), Some(base)) = (
            reg_string(&sub, w!("DistributionName")),
            reg_string(&sub, w!("BasePath")),
        ) {
            out.push(Distro {
                name,
                vhdx: format!("{}\\ext4.vhdx", base.trim_end_matches('\\')),
            });
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    out
}

/// Raiz UNC de una distro para medirla desde Windows.
pub fn wsl_root(name: &str) -> String {
    format!("\\\\wsl.localhost\\{name}\\")
}

/// Nombre de proceso de la maquina virtual de WSL 2 (sin distinguir mayusculas).
pub fn is_wsl_vm(exe: &str) -> bool {
    exe.to_ascii_lowercase().starts_with("vmmemwsl")
}

/// WSL 2 esta corriendo si existe su proceso vmmemWSL.
pub fn wsl_running() -> bool {
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        if is_wsl_vm(&String::from_utf16_lossy(&entry.szExeFile[..len])) {
            found = true;
            break;
        }
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snap);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn porcentaje_ocupado() {
        let s = Space {
            total: 100 * GIB,
            free: 25 * GIB,
        };
        assert_eq!(s.used(), 75 * GIB);
        assert_eq!(s.percent(), 75.0);
        assert_eq!(Space { total: 0, free: 0 }.percent(), 0.0);
        assert_eq!(Space { total: 1, free: 5 }.used(), 0);
    }

    #[test]
    fn texto_en_gb_o_mb() {
        let grande = Space {
            total: 237 * GIB + GIB / 2,
            free: 24 * GIB,
        };
        assert_eq!(space_text(&grande), "213.5 / 237.5 GB");
        const MIB: u64 = 1024 * 1024;
        let chica = Space {
            total: 32 * MIB,
            free: 4 * MIB,
        };
        assert_eq!(space_text(&chica), "28.0 / 32.0 MB");
        assert_eq!(size_short(116 * GIB), "116 GB");
        assert_eq!(size_short(33_277_440), "32 MB");
    }

    #[test]
    fn letras_de_la_mascara() {
        // A, C y D.
        assert_eq!(letters(0b1101).collect::<Vec<_>>(), vec!['A', 'C', 'D']);
        assert_eq!(letters(1 << 25).collect::<Vec<_>>(), vec!['Z']);
        assert_eq!(letters(0).count(), 0);
    }

    #[test]
    fn bit_de_letra() {
        assert_eq!(bit('C'), 0b100);
        assert_eq!(bit('c'), 0b100);
        assert_eq!(bit('Z'), 1 << 25);
        assert_eq!(bit('1'), 0);
        for l in letters(0x3ff_ffff) {
            assert_eq!(letters(bit(l)).collect::<Vec<_>>(), vec![l]);
        }
    }

    #[test]
    fn rutas() {
        assert_eq!(root('C'), "C:\\");
        assert_eq!(
            wsl_root("Ubuntu-24.04"),
            "\\\\wsl.localhost\\Ubuntu-24.04\\"
        );
    }

    #[test]
    fn proceso_de_wsl() {
        assert!(is_wsl_vm("vmmemWSL"));
        assert!(is_wsl_vm("VMMEMWSL.exe"));
        assert!(!is_wsl_vm("vmmem"));
        assert!(!is_wsl_vm("wsl.exe"));
    }

    #[test]
    fn lee_el_disco_del_sistema() {
        let drives = drives();
        assert!(drives.contains(&'C'), "sin C: en {drives:?}");
        let s = space(&root('C')).expect("espacio de C:");
        assert!(s.total > 0 && s.free <= s.total);
    }
}
