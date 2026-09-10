pub mod clean;
pub mod commands;
pub mod rules;
pub mod scan;
pub mod startup;
pub mod winapp2;

/// Text shown when the embedded rules refuse to load. In release,
/// `windows_subsystem = "windows"` means standard error is visible nowhere, so
/// the message has to go through a dialog box.
pub fn startup_error_message(err: &rules::RuleError) -> String {
    format!(
        "WinCleaner cannot start.\n\n\
         The embedded rules file (rules.toml) is invalid:\n{err}\n\n\
         No file has been touched. The application will now close."
    )
}

/// Shows the startup failure message. In release the binary has no console:
/// without a dialog box, the application would close silently.
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
    // Blocking load: an invalid rules.toml forbids startup.
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
        .expect("error while launching WinCleaner");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleError;

    #[test]
    fn the_startup_gate_message_carries_the_error() {
        let msg = startup_error_message(&RuleError::UnknownVar("WINDIR".to_string()));
        assert!(msg.starts_with("WinCleaner cannot start"));
        assert!(msg.contains("rules.toml"));
        assert!(msg.contains("%WINDIR%"));
    }
}
