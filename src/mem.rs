//! Lectura de la memoria (GlobalMemoryStatusEx) y la logica pura que la convierte en texto,
//! nivel de color e historial. Todo lo que no toca Win32 tiene pruebas.

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Muestras guardadas para la grafica: 60 x 2 s = los ultimos 2 minutos.
pub const HISTORY: usize = 60;
/// Desde aqui la barra pasa a ambar.
pub const HIGH: f64 = 75.0;
/// Desde aqui la barra pasa a rojo.
pub const CRITICAL: f64 = 90.0;

/// Una lectura, en bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mem {
    pub total: u64,
    pub avail: u64,
    /// Memoria confirmada (commit) en uso y su limite (RAM + archivo de paginacion).
    pub commit_used: u64,
    pub commit_limit: u64,
}

impl Mem {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.avail)
    }

    /// Porcentaje de RAM en uso, en [0, 100].
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.used() as f64 * 100.0 / self.total as f64).clamp(0.0, 100.0)
    }
}

pub fn read() -> Option<Mem> {
    let mut st = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut st) }.ok()?;
    Some(Mem {
        total: st.ullTotalPhys,
        avail: st.ullAvailPhys,
        commit_used: st.ullTotalPageFile.saturating_sub(st.ullAvailPageFile),
        commit_limit: st.ullTotalPageFile,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Normal,
    High,
    Critical,
}

pub fn level(percent: f64) -> Level {
    if percent >= CRITICAL {
        Level::Critical
    } else if percent >= HIGH {
        Level::High
    } else {
        Level::Normal
    }
}

/// Bytes a GB (base 1024, como el Administrador de tareas) con un decimal.
pub fn gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// "68%". Redondea, pero nunca dice 100% si no esta llena del todo.
pub fn percent_text(p: f64) -> String {
    let r = p.round();
    let r = if r >= 100.0 && p < 100.0 { 99.0 } else { r };
    format!("{}%", r.clamp(0.0, 100.0) as u32)
}

/// Anillo con las ultimas `HISTORY` muestras (porcentajes).
pub struct History {
    buf: [f32; HISTORY],
    len: usize,
    next: usize,
}

impl History {
    pub const fn new() -> Self {
        Self {
            buf: [0.0; HISTORY],
            len: 0,
            next: 0,
        }
    }

    pub fn push(&mut self, p: f64) {
        self.buf[self.next] = p as f32;
        self.next = (self.next + 1) % HISTORY;
        self.len = (self.len + 1).min(HISTORY);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// De la mas vieja a la mas nueva.
    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        let start = (self.next + HISTORY - self.len) % HISTORY;
        (0..self.len).map(move |i| self.buf[(start + i) % HISTORY] as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn porcentaje_y_usada() {
        let m = Mem {
            total: 16 * GIB,
            avail: 4 * GIB,
            commit_used: 0,
            commit_limit: 0,
        };
        assert_eq!(m.used(), 12 * GIB);
        assert_eq!(m.percent(), 75.0);
    }

    #[test]
    fn total_cero_no_divide() {
        let m = Mem {
            total: 0,
            avail: 0,
            commit_used: 0,
            commit_limit: 0,
        };
        assert_eq!(m.percent(), 0.0);
    }

    #[test]
    fn disponible_mayor_que_total_no_desborda() {
        let m = Mem {
            total: GIB,
            avail: 2 * GIB,
            commit_used: 0,
            commit_limit: 0,
        };
        assert_eq!(m.used(), 0);
        assert_eq!(m.percent(), 0.0);
    }

    #[test]
    fn niveles() {
        assert_eq!(level(0.0), Level::Normal);
        assert_eq!(level(74.9), Level::Normal);
        assert_eq!(level(75.0), Level::High);
        assert_eq!(level(89.9), Level::High);
        assert_eq!(level(90.0), Level::Critical);
        assert_eq!(level(100.0), Level::Critical);
    }

    #[test]
    fn gb_con_un_decimal() {
        assert_eq!(gb(16 * GIB), "16.0");
        assert_eq!(gb(GIB + GIB / 2), "1.5");
        assert_eq!(gb(0), "0.0");
    }

    #[test]
    fn texto_de_porcentaje() {
        assert_eq!(percent_text(68.4), "68%");
        assert_eq!(percent_text(68.5), "69%");
        assert_eq!(percent_text(99.7), "99%");
        assert_eq!(percent_text(100.0), "100%");
        assert_eq!(percent_text(0.2), "0%");
    }

    #[test]
    fn historial_en_orden_y_con_tope() {
        let mut h = History::new();
        assert_eq!(h.len(), 0);
        for i in 0..3 {
            h.push(i as f64);
        }
        assert_eq!(h.iter().collect::<Vec<_>>(), vec![0.0, 1.0, 2.0]);
        for i in 3..(HISTORY + 5) {
            h.push(i as f64);
        }
        assert_eq!(h.len(), HISTORY);
        let v: Vec<f64> = h.iter().collect();
        assert_eq!(v.first(), Some(&5.0));
        assert_eq!(v.last(), Some(&((HISTORY + 4) as f64)));
    }
}
