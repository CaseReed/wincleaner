pub mod clean;
pub mod commands;
pub mod rules;
pub mod scan;
pub mod startup;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Chargement bloquant : un rules.toml invalide interdit le démarrage.
    if let Err(err) = rules::embedded_rules() {
        eprintln!("WinCleaner ne peut pas démarrer : {err}");
        std::process::exit(1);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_rules,
            commands::scan,
            commands::clean,
            commands::running_browsers,
            commands::list_startup,
            commands::set_startup_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de WinCleaner");
}
