//! Textos en espanol o ingles segun el idioma de la interfaz de Windows.

use windows::Win32::Globalization::GetUserDefaultUILanguage;

pub struct Texts {
    pub free: &'static str,
    pub commit: &'static str,
    pub toggle: &'static str,
    pub topmost: &'static str,
    pub autostart: &'static str,
    pub taskmgr: &'static str,
    pub quit: &'static str,
}

const ES: Texts = Texts {
    free: "Libre",
    commit: "Confirmada",
    toggle: "Mostrar u ocultar",
    topmost: "Siempre encima",
    autostart: "Iniciar con Windows",
    taskmgr: "Administrador de tareas",
    quit: "Salir",
};

const EN: Texts = Texts {
    free: "Free",
    commit: "Committed",
    toggle: "Show or hide",
    topmost: "Always on top",
    autostart: "Start with Windows",
    taskmgr: "Task Manager",
    quit: "Quit",
};

/// LANGID primario de espanol (LANG_SPANISH).
const LANG_SPANISH: u16 = 0x0a;

pub fn for_langid(langid: u16) -> &'static Texts {
    if langid & 0x3ff == LANG_SPANISH {
        &ES
    } else {
        &EN
    }
}

pub fn current() -> &'static Texts {
    for_langid(unsafe { GetUserDefaultUILanguage() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn espanol_de_cualquier_pais() {
        // es-MX (0x080a), es-ES (0x0c0a), es-AR (0x2c0a).
        for id in [0x080a, 0x0c0a, 0x2c0a] {
            assert_eq!(for_langid(id).quit, "Salir");
        }
    }

    #[test]
    fn lo_demas_en_ingles() {
        // en-US (0x0409), pt-BR (0x0416).
        for id in [0x0409, 0x0416, 0] {
            assert_eq!(for_langid(id).quit, "Quit");
        }
    }
}
