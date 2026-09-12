// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // An argument on the command line means the read-only CLI (`cli.rs`),
    // answered here — before `tauri::Builder` — so `--analyze` opens no
    // window, costs no WebView2 and no message loop. No argument at all is
    // the application, which is what a double click and every shortcut send.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        wincleaner_lib::run();
    } else {
        std::process::exit(wincleaner_lib::cli::main(&args));
    }
}
