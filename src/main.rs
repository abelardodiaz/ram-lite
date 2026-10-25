//! ram-lite: widget nativo minimo para Windows con el uso de memoria RAM, Win32 + GDI.
//! Sin WebView, sin hilos y sin red: un temporizador cada 2 s llama a GlobalMemoryStatusEx.
//! Estilo y patrones de ventana tomados de la version lite de claude-usage-widgets.

#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("ram-lite es solo para Windows");

mod app;
mod autostart;
mod icon;
mod mem;
mod placement;
mod settings;
mod text;

// Paleta de claude-usage-widgets (desktop/ui/style.css), 0xRRGGBB.
const BG: u32 = 0x16181d;
const TEXT: u32 = 0xe7e9ee;
const MUTED: u32 = 0x8b919c;
const DIM: u32 = 0x5f6570;
const FAINT: u32 = 0x838993;
const TRACK: u32 = 0x262a32;
const LINE: u32 = 0x2a2e37;
const GREEN: u32 = 0x3fbf6f;
const AMBER: u32 = 0xe8b03f;
const RED: u32 = 0xe5534b;
/// Fondo de la X cuando el raton esta encima.
const CLOSE_HOT: u32 = 0x3a3f4a;

fn main() {
    app::run();
}
