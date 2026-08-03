// Binary entrypoint. Semua logika di lib.rs; ini cukup panggil run.
// `windows_subsystem` penting: tanpanya exe compile ke subsystem CONSOLE →
// Windows buka terminal hitam tambahan setiap app dijalankan. Wajib GUI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    zpos_kasir_lib::run_app();
}
