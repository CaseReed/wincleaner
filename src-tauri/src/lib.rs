pub mod clean;
pub mod commands;
pub mod rules;
pub mod scan;
pub mod startup;

/// Texte affiché quand les règles embarquées refusent de se charger. En
/// release `windows_subsystem = "windows"` : la sortie d'erreur n'est visible
/// nulle part, le message doit passer par une boîte de dialogue.
pub fn startup_error_message(err: &rules::RuleError) -> String {
    format!(
        "WinCleaner ne peut pas démarrer.\n\n\
         Le fichier de règles embarqué (rules.toml) est invalide :\n{err}\n\n\
         Aucun fichier n'a été touché. L'application va se fermer."
    )
}

/// Affiche le message d'échec de démarrage. En release le binaire n'a pas de
/// console : sans boîte de dialogue, l'application se fermerait en silence.
fn show_startup_error(message: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "WinCleaner"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Chargement bloquant : un rules.toml invalide interdit le démarrage.
    if let Err(err) = rules::embedded_rules() {
        let message = startup_error_message(&err);
        eprintln!("{message}");
        show_startup_error(&message);
        std::process::exit(1);
    }

    tauri::Builder::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleError;

    #[test]
    fn le_message_de_la_porte_de_demarrage_reprend_lerreur() {
        let msg = startup_error_message(&RuleError::UnknownVar("WINDIR".to_string()));
        assert!(msg.starts_with("WinCleaner ne peut pas démarrer"));
        assert!(msg.contains("rules.toml"));
        assert!(msg.contains("%WINDIR%"));
    }
}
