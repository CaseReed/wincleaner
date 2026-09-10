use crate::rules::system_env;
use serde::{Deserialize, Serialize};
use std::fmt;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_BINARY};
use winreg::{RegKey, RegValue};

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
pub const APPROVED_RUN_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
pub const APPROVED_FOLDER_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartupSource {
    Run,
    RunOnce,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub source: StartupSource,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupError {
    /// Malformed entry id.
    BadId(String),
    /// `RunOnce` is read-only in the MVP.
    ReadOnlySource,
    /// No entry carries this id.
    NotFound(String),
    /// Registry or file system access error.
    Io(String),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartupError::BadId(id) => write!(f, "invalid startup entry id: \"{id}\""),
            StartupError::ReadOnlySource => write!(
                f,
                "RunOnce entries are read-only and cannot be disabled"
            ),
            StartupError::NotFound(id) => write!(f, "no startup entry \"{id}\""),
            StartupError::Io(m) => write!(f, "cannot access startup entries: {m}"),
        }
    }
}

impl std::error::Error for StartupError {}

/// StartupApproved blob: 12 bytes.
/// byte 0: 0x02 enabled, 0x03 disabled.
/// bytes 1..4: zero.
/// bytes 4..12: FILETIME of the deactivation, little-endian (0 when enabled).
pub fn encode_startup_state(enabled: bool, filetime: u64) -> [u8; 12] {
    let mut blob = [0u8; 12];
    blob[0] = if enabled { 0x02 } else { 0x03 };
    let ft = if enabled { 0 } else { filetime };
    blob[4..12].copy_from_slice(&ft.to_le_bytes());
    blob
}

/// Missing value (empty slice) or a blob shorter than 12 bytes: enabled.
///
/// Windows encodes the state in bit 0 of byte 0: `0x02`, `0x06`, `0x0A` are
/// enabled, `0x03`, `0x07`, `0x0B` disabled. Values other than `0x02`/`0x03`
/// are written by other tools (Task Manager, Autoruns) and must be read
/// correctly.
pub fn decode_startup_state(blob: &[u8]) -> bool {
    if blob.len() < 12 {
        return true;
    }
    blob[0] & 1 == 0
}

fn source_tag(source: StartupSource) -> &'static str {
    match source {
        StartupSource::Run => "run",
        StartupSource::RunOnce => "run-once",
        StartupSource::Folder => "folder",
    }
}

pub fn entry_id(source: StartupSource, name: &str) -> String {
    format!("{}:{}", source_tag(source), name)
}

pub fn parse_entry_id(id: &str) -> Result<(StartupSource, String), StartupError> {
    let (tag, name) = id
        .split_once(':')
        .ok_or_else(|| StartupError::BadId(id.to_string()))?;
    if name.is_empty() {
        return Err(StartupError::BadId(id.to_string()));
    }
    let source = match tag {
        "run" => StartupSource::Run,
        "run-once" => StartupSource::RunOnce,
        "folder" => StartupSource::Folder,
        _ => return Err(StartupError::BadId(id.to_string())),
    };
    Ok((source, name.to_string()))
}

/// Reads a binary value under HKCU. `Ok(None)` when the key or the value does
/// not exist — which, for StartupApproved, means "enabled".
pub fn read_hkcu_binary(subkey: &str, value_name: &str) -> Result<Option<Vec<u8>>, StartupError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(subkey, KEY_READ) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StartupError::Io(e.to_string())),
    };
    match key.get_raw_value(value_name) {
        Ok(v) => Ok(Some(v.bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StartupError::Io(e.to_string())),
    }
}

/// Writes a binary value under HKCU, creating the subkey if needed.
pub fn write_hkcu_binary(subkey: &str, value_name: &str, blob: &[u8]) -> Result<(), StartupError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(subkey, KEY_SET_VALUE) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => hkcu
            .create_subkey(subkey)
            .map_err(|e| StartupError::Io(e.to_string()))?
            .0,
        Err(e) => return Err(StartupError::Io(e.to_string())),
    };
    let value = RegValue {
        bytes: blob.to_vec(),
        vtype: REG_BINARY,
    };
    key.set_raw_value(value_name, &value)
        .map_err(|e| StartupError::Io(e.to_string()))
}

/// Current FILETIME: hundreds of nanoseconds since 1 January 1601.
pub fn now_filetime() -> u64 {
    const UNIX_EPOCH_IN_FILETIME: u64 = 116_444_736_000_000_000;
    let since_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 / 100)
        .unwrap_or(0);
    UNIX_EPOCH_IN_FILETIME + since_unix
}

fn read_run_values(subkey: &str) -> Result<Vec<(String, String)>, StartupError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(subkey, KEY_READ) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(StartupError::Io(e.to_string())),
    };
    let mut out = Vec::new();
    for item in key.enum_values() {
        let (name, value) = item.map_err(|e| StartupError::Io(e.to_string()))?;
        if name.is_empty() {
            continue;
        }
        out.push((name, value.to_string()));
    }
    Ok(out)
}

fn startup_folder() -> Option<std::path::PathBuf> {
    let appdata = system_env("APPDATA")?;
    Some(
        std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup"),
    )
}

fn approved_key_for(source: StartupSource) -> &'static str {
    match source {
        StartupSource::Folder => APPROVED_FOLDER_KEY,
        _ => APPROVED_RUN_KEY,
    }
}

fn is_enabled(source: StartupSource, name: &str) -> Result<bool, StartupError> {
    let blob = read_hkcu_binary(approved_key_for(source), name)?;
    Ok(match blob {
        None => true,
        Some(bytes) => decode_startup_state(&bytes),
    })
}

pub fn list_startup() -> Result<Vec<StartupEntry>, StartupError> {
    let mut entries: Vec<StartupEntry> = Vec::new();

    for (name, command) in read_run_values(RUN_KEY)? {
        let enabled = is_enabled(StartupSource::Run, &name)?;
        entries.push(StartupEntry {
            id: entry_id(StartupSource::Run, &name),
            name: name.clone(),
            command,
            source: StartupSource::Run,
            enabled,
        });
    }

    for (name, command) in read_run_values(RUN_ONCE_KEY)? {
        entries.push(StartupEntry {
            id: entry_id(StartupSource::RunOnce, &name),
            name: name.clone(),
            command,
            // RunOnce has no StartupApproved state: always shown as enabled.
            source: StartupSource::RunOnce,
            enabled: true,
        });
    }

    if let Some(folder) = startup_folder() {
        if let Ok(read_dir) = std::fs::read_dir(&folder) {
            for item in read_dir.flatten() {
                let file_name = item.file_name().to_string_lossy().to_string();
                if file_name.eq_ignore_ascii_case("desktop.ini") {
                    continue;
                }
                if !item.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                let enabled = is_enabled(StartupSource::Folder, &file_name)?;
                entries.push(StartupEntry {
                    id: entry_id(StartupSource::Folder, &file_name),
                    name: file_name.clone(),
                    command: item.path().to_string_lossy().to_string(),
                    source: StartupSource::Folder,
                    enabled,
                });
            }
        }
    }

    entries.sort_by_key(|a| a.name.to_lowercase());
    Ok(entries)
}

/// Enables or disables an entry by writing its StartupApproved blob. The entry
/// is never deleted. `RunOnce` is refused.
pub fn set_startup_enabled(id: &str, enabled: bool) -> Result<(), StartupError> {
    let (source, name) = parse_entry_id(id)?;
    if source == StartupSource::RunOnce {
        return Err(StartupError::ReadOnlySource);
    }
    let exists = list_startup()?.into_iter().any(|e| e.id == id);
    if !exists {
        return Err(StartupError::NotFound(id.to_string()));
    }
    let blob = encode_startup_state(enabled, now_filetime());
    write_hkcu_binary(approved_key_for(source), &name, &blob)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    /// Test subkey, created and deleted by the test itself.
    /// Never point this at the real Run keys.
    const TEST_KEY: &str = r"Software\wincleaner-test";

    struct ScratchTestKey(String);

    impl ScratchTestKey {
        fn new(suffix: &str) -> Self {
            let path = format!("{TEST_KEY}\\{suffix}");
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            hkcu.create_subkey(&path).unwrap();
            ScratchTestKey(path)
        }
    }

    impl Drop for ScratchTestKey {
        fn drop(&mut self) {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let _ = hkcu.delete_subkey_all(&self.0);
            let _ = hkcu.delete_subkey(TEST_KEY);
        }
    }

    #[test]
    fn encoding_enabled_writes_0x02_and_a_zero_filetime() {
        let blob = encode_startup_state(true, 0);
        assert_eq!(blob.len(), 12);
        assert_eq!(blob[0], 0x02);
        assert_eq!(&blob[1..4], &[0, 0, 0]);
        assert_eq!(&blob[4..12], &[0u8; 8]);
    }

    #[test]
    fn encoding_disabled_writes_0x03_and_the_filetime_little_endian() {
        let blob = encode_startup_state(false, 0x0102_0304_0506_0708);
        assert_eq!(blob[0], 0x03);
        assert_eq!(&blob[1..4], &[0, 0, 0]);
        assert_eq!(
            &blob[4..12],
            &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn decoding_a_missing_value_means_enabled() {
        assert!(decode_startup_state(&[]));
    }

    #[test]
    fn decoding_a_too_short_blob_means_enabled() {
        assert!(decode_startup_state(&[0x03, 0x00, 0x00]));
    }

    #[test]
    fn decoding_0x03_means_disabled() {
        let blob = encode_startup_state(false, 42);
        assert!(!decode_startup_state(&blob));
    }

    #[test]
    fn decoding_0x02_means_enabled() {
        let blob = encode_startup_state(true, 0);
        assert!(decode_startup_state(&blob));
    }

    #[test]
    fn decoding_0x06_written_by_another_tool_means_enabled() {
        let mut blob = [0u8; 12];
        blob[0] = 0x06;
        assert!(decode_startup_state(&blob));
    }

    #[test]
    fn decoding_0x07_written_by_another_tool_means_disabled() {
        // Windows and Task Manager encode the state in bit 0:
        // 0x02/0x06/0x0A are enabled, 0x03/0x07/0x0B disabled.
        for first in [0x03u8, 0x07, 0x0B] {
            let mut blob = [0u8; 12];
            blob[0] = first;
            assert!(
                !decode_startup_state(&blob),
                "0x{first:02X} must be read as disabled"
            );
        }
        for first in [0x02u8, 0x06, 0x0A] {
            let mut blob = [0u8; 12];
            blob[0] = first;
            assert!(
                decode_startup_state(&blob),
                "0x{first:02X} must be read as enabled"
            );
        }
    }

    #[test]
    fn encoding_then_decoding_is_an_identity() {
        assert!(decode_startup_state(&encode_startup_state(true, 0)));
        assert!(!decode_startup_state(&encode_startup_state(
            false,
            now_filetime()
        )));
    }

    #[test]
    fn entry_ids_round_trip() {
        assert_eq!(entry_id(StartupSource::Run, "OneDrive"), "run:OneDrive");
        assert_eq!(entry_id(StartupSource::RunOnce, "Patch"), "run-once:Patch");
        assert_eq!(
            entry_id(StartupSource::Folder, "Notes.lnk"),
            "folder:Notes.lnk"
        );
        assert_eq!(
            parse_entry_id("run:OneDrive").unwrap(),
            (StartupSource::Run, "OneDrive".to_string())
        );
        assert_eq!(
            parse_entry_id("folder:My App.lnk").unwrap(),
            (StartupSource::Folder, "My App.lnk".to_string())
        );
    }

    #[test]
    fn parsing_an_invalid_id_fails() {
        assert!(matches!(
            parse_entry_id("bogus:X").unwrap_err(),
            StartupError::BadId(_)
        ));
        assert!(matches!(
            parse_entry_id("run").unwrap_err(),
            StartupError::BadId(_)
        ));
    }

    #[test]
    fn writing_then_reading_a_blob_in_hkcu() {
        let key = ScratchTestKey::new("approved");
        let blob = encode_startup_state(false, 0x1122_3344_5566_7788);

        assert_eq!(read_hkcu_binary(&key.0, "MyApp").unwrap(), None);

        write_hkcu_binary(&key.0, "MyApp", &blob).unwrap();
        let read_back = read_hkcu_binary(&key.0, "MyApp").unwrap().unwrap();
        assert_eq!(read_back, blob.to_vec());
        assert!(!decode_startup_state(&read_back));

        write_hkcu_binary(&key.0, "MyApp", &encode_startup_state(true, 0)).unwrap();
        let read_back = read_hkcu_binary(&key.0, "MyApp").unwrap().unwrap();
        assert!(decode_startup_state(&read_back));
    }

    #[test]
    fn reading_a_missing_key_returns_none() {
        let missing = r"Software\wincleaner-test\never-created";
        assert_eq!(read_hkcu_binary(missing, "X").unwrap(), None);
    }

    #[test]
    fn disabling_a_run_once_entry_is_refused() {
        let err = set_startup_enabled("run-once:Patch", false).unwrap_err();
        assert!(matches!(err, StartupError::ReadOnlySource));
    }

    /// Environment-coupled test: it reads the real Run/RunOnce keys and the
    /// real Startup folder of the current user (read-only, nothing is
    /// modified). The list therefore depends on the machine: the assertions
    /// only cover shape invariants, never expected content.
    #[test]
    fn listing_startup_entries_does_not_panic() {
        let entries = list_startup().unwrap();
        for e in &entries {
            assert!(!e.id.is_empty());
            assert!(!e.name.is_empty());
            assert!(parse_entry_id(&e.id).is_ok());
        }
    }
}
