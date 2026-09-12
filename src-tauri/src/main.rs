// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // An argument on the command line means the read-only CLI (`cli.rs`),
    // answered here — before `tauri::Builder` — so `--analyze` opens no
    // window, costs no WebView2 and no message loop. No argument at all is
    // the application, which is what a double click and every shortcut send.
    // `args_os`, not `args`: the latter panics on an argument that is not
    // valid Unicode, and this binary is windowed — the panic would have no
    // console to print to. `cli::main` answers such an argument with the
    // usage error it is (`cli::to_utf8`).
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.is_empty() {
        wincleaner_lib::run();
    } else {
        std::process::exit(wincleaner_lib::cli::main(&args));
    }
}
