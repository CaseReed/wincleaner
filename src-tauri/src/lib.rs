pub mod clean;
pub mod commands;
pub mod exclusions;
pub mod rules;
pub mod sandbox;
pub mod scan;
pub mod startup;
pub mod update;
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

/// How long the window close waits for the sandbox to be removed. Long enough
/// for a few hundred small files under `%TEMP%`, short enough that a user who
/// clicks the cross does not think the application has hung: past it, the
/// close goes through and the leftover is the next start's problem.
const CLOSE_LEAVE_BUDGET: std::time::Duration = std::time::Duration::from_millis(1_500);

/// Best-effort removal of the active sandbox when the window closes.
///
/// **Guaranteed**: the application never *chooses* to leave a sandbox behind.
/// A close with a sandbox open attempts the same `leave_sandbox` the Settings
/// button calls, with the same junction-safe removal.
///
/// **Not guaranteed**: that it succeeds. The removal may exceed the budget
/// above (a slow volume, a file still open under the root), the process may be
/// killed outright, or it may crash — none of which reaches this function at
/// all. That is why the startup sweep in `run` exists and is the real safety
/// net: this only spares the user one restart's worth of leftover.
///
/// The work runs on its own thread with a bounded wait rather than inline: a
/// `remove_dir_all` that blocks would freeze the window on the way out, which
/// is exactly the impression a cleaner cannot afford to give.
fn leave_on_close(window: &tauri::Window) {
    use tauri::Manager;

    // `CloseRequested` is followed by `Destroyed`; one attempt is enough, and
    // a second would spend the budget again for nothing.
    static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    let state = window.state::<commands::SandboxState>().inner().clone();
    if commands::sandbox_status_of(&state).is_none() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(commands::leave_sandbox(&state));
    });
    match rx.recv_timeout(CLOSE_LEAVE_BUDGET) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => eprintln!("sandbox not removed on close: {err}"),
        Err(_) => eprintln!(
            "sandbox removal did not finish within {} ms; the next start will sweep it",
            CLOSE_LEAVE_BUDGET.as_millis()
        ),
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

    // Parsing the embedded Winapp2 base and probing the registry costs about a
    // second. Started here, off the main thread, so the window opens at once
    // and the first `list_rules` finds the catalogue already built.
    tauri::async_runtime::spawn_blocking(|| {
        if let Err(err) = commands::catalogue() {
            // Not fatal: the native rules alone still make a usable
            // application, and `list_rules` will report this same error.
            eprintln!("catalogue: {err}");
        }
    });

    // The safety net for a sandbox whose process died before `leave`: a crash,
    // a kill, a close the window event below could not finish. Off the main
    // thread — it reads and deletes a few hundred files — and after the
    // catalogue, which is what the first screen waits on.
    tauri::async_runtime::spawn_blocking(|| {
        let removed = commands::sweep_orphans();
        if removed > 0 {
            let plural = if removed == 1 { "y" } else { "ies" };
            eprintln!("swept {removed} orphaned sandbox director{plural} from %TEMP%");
        }
    });

    tauri::Builder::default()
        // Empty until the user asks for a sandbox in Settings: the engine runs
        // against the real profile, as it always has.
        .manage(commands::SandboxState::default())
        // Paths of the last analysis, per rule: what lets `add_exclusion` take
        // an index instead of a path.
        .manage(commands::LastPaths::default())
        .on_window_event(|window, event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                leave_on_close(window);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_rules,
            commands::rules_summary,
            commands::scan,
            commands::clean,
            commands::running_browsers,
            commands::list_startup,
            commands::set_startup_enabled,
            commands::check_for_updates,
            commands::add_exclusion,
            commands::list_exclusions,
            commands::remove_exclusion,
            commands::sandbox_enter,
            commands::sandbox_leave,
            commands::sandbox_status,
            commands::sandbox_verify,
            commands::sandbox_orphans,
            commands::sandbox_remove_orphans,
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
