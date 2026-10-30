//! Uso de CPU (GetSystemTimes): se compara cada lectura con la anterior. Solo se lee si el usuario
//! encendio "Mostrar CPU"; apagado no cuesta nada. La cuenta es pura y tiene pruebas.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::GetSystemTimes;

/// Tiempos acumulados de todos los nucleos, en unidades de 100 ns. `kernel` incluye `idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Times {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

fn ticks(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

pub fn read() -> Option<Times> {
    let (mut idle, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    Some(Times {
        idle: ticks(idle),
        kernel: ticks(kernel),
        user: ticks(user),
    })
}

/// Porcentaje de CPU ocupada entre dos lecturas, en [0, 100]. `None` si no paso tiempo o los
/// contadores retrocedieron.
pub fn percent(prev: Times, now: Times) -> Option<f64> {
    let idle = now.idle.checked_sub(prev.idle)?;
    let total = now.kernel.checked_sub(prev.kernel)? + now.user.checked_sub(prev.user)?;
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some((busy as f64 * 100.0 / total as f64).clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(idle: u64, kernel: u64, user: u64) -> Times {
        Times { idle, kernel, user }
    }

    #[test]
    fn mitad_ocupada() {
        // 100 de kernel (60 de ellos idle) + 20 de usuario = 120; ocupado 60 -> 50%.
        assert_eq!(percent(t(0, 0, 0), t(60, 100, 20)), Some(50.0));
    }

    #[test]
    fn todo_idle_es_cero() {
        assert_eq!(percent(t(10, 10, 0), t(110, 110, 0)), Some(0.0));
    }

    #[test]
    fn sin_tiempo_o_retroceso_no_da_valor() {
        let a = t(5, 10, 3);
        assert_eq!(percent(a, a), None);
        assert_eq!(percent(t(50, 100, 10), t(40, 90, 10)), None);
    }

    #[test]
    fn idle_mayor_que_total_no_desborda() {
        assert_eq!(percent(t(0, 0, 0), t(200, 100, 0)), Some(0.0));
    }

    #[test]
    fn lee_del_sistema() {
        let a = read().expect("GetSystemTimes");
        assert!(a.kernel >= a.idle);
    }
}
