use crate::rules::{system_env, EnvLookup};
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
    /// Identifiant d'entrée mal formé.
    BadId(String),
    /// `RunOnce` est en lecture seule dans le MVP.
    ReadOnlySource,
    /// Aucune entrée ne porte cet identifiant.
    NotFound(String),
    /// Erreur d'accès au registre ou au système de fichiers.
    Io(String),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartupError::BadId(id) => write!(f, "identifiant d'entrée invalide : « {id} »"),
            StartupError::ReadOnlySource => write!(
                f,
                "les entrées RunOnce sont en lecture seule et ne peuvent pas être désactivées"
            ),
            StartupError::NotFound(id) => write!(f, "aucune entrée de démarrage « {id} »"),
            StartupError::Io(m) => write!(f, "accès au démarrage impossible : {m}"),
        }
    }
}

impl std::error::Error for StartupError {}

/// Blob StartupApproved : 12 octets.
/// octet 0 : 0x02 activé, 0x03 désactivé.
/// octets 1..4 : zéro.
/// octets 4..12 : FILETIME de la désactivation, petit-boutien (0 si activé).
pub fn encode_startup_state(enabled: bool, filetime: u64) -> [u8; 12] {
    let mut blob = [0u8; 12];
    blob[0] = if enabled { 0x02 } else { 0x03 };
    let ft = if enabled { 0 } else { filetime };
    blob[4..12].copy_from_slice(&ft.to_le_bytes());
    blob
}

/// Absence de valeur (slice vide) ou blob de moins de 12 octets : activé.
///
/// Windows code l'état dans le bit 0 de l'octet 0 : `0x02`, `0x06`, `0x0A`
/// sont activés, `0x03`, `0x07`, `0x0B` désactivés. Les valeurs autres que
/// `0x02`/`0x03` sont écrites par d'autres outils (Gestionnaire des tâches,
/// Autoruns) et doivent être lues correctement.
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

/// Lit une valeur binaire sous HKCU. `Ok(None)` si la clé ou la valeur
/// n'existe pas — ce qui, pour StartupApproved, signifie « activé ».
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

/// Écrit une valeur binaire sous HKCU, en créant la sous-clé au besoin.
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

/// FILETIME courant : centaines de nanosecondes depuis le 1er janvier 1601.
pub fn now_filetime() -> u64 {
    const UNIX_EPOCH_EN_FILETIME: u64 = 116_444_736_000_000_000;
    let since_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 / 100)
        .unwrap_or(0);
    UNIX_EPOCH_EN_FILETIME + since_unix
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

fn startup_folder(lookup: EnvLookup) -> Option<std::path::PathBuf> {
    let appdata = lookup("APPDATA")?;
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

pub fn list_startup_with(lookup: EnvLookup) -> Result<Vec<StartupEntry>, StartupError> {
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
            // RunOnce n'a pas d'état StartupApproved : toujours affiché actif.
            source: StartupSource::RunOnce,
            enabled: true,
        });
    }

    if let Some(folder) = startup_folder(lookup) {
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

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

pub fn list_startup() -> Result<Vec<StartupEntry>, StartupError> {
    list_startup_with(&system_env)
}

/// Active ou désactive une entrée en écrivant son blob StartupApproved.
/// L'entrée n'est jamais supprimée. `RunOnce` est refusé.
pub fn set_startup_enabled(id: &str, enabled: bool) -> Result<(), StartupError> {
    let (source, name) = parse_entry_id(id)?;
    if source == StartupSource::RunOnce {
        return Err(StartupError::ReadOnlySource);
    }
    let existe = list_startup()?.into_iter().any(|e| e.id == id);
    if !existe {
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

    /// Sous-clé de test, créée et supprimée par le test lui-même.
    /// Ne jamais pointer sur les vraies clés Run.
    const TEST_KEY: &str = r"Software\wincleaner-test";

    struct CleClaireDeTest(String);

    impl CleClaireDeTest {
        fn new(suffix: &str) -> Self {
            let path = format!("{TEST_KEY}\\{suffix}");
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            hkcu.create_subkey(&path).unwrap();
            CleClaireDeTest(path)
        }
    }

    impl Drop for CleClaireDeTest {
        fn drop(&mut self) {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let _ = hkcu.delete_subkey_all(&self.0);
            let _ = hkcu.delete_subkey(TEST_KEY);
        }
    }

    #[test]
    fn encode_active_met_0x02_et_un_filetime_nul() {
        let blob = encode_startup_state(true, 0);
        assert_eq!(blob.len(), 12);
        assert_eq!(blob[0], 0x02);
        assert_eq!(&blob[1..4], &[0, 0, 0]);
        assert_eq!(&blob[4..12], &[0u8; 8]);
    }

    #[test]
    fn encode_desactive_met_0x03_et_le_filetime_en_petit_boutien() {
        let blob = encode_startup_state(false, 0x0102_0304_0506_0708);
        assert_eq!(blob[0], 0x03);
        assert_eq!(&blob[1..4], &[0, 0, 0]);
        assert_eq!(
            &blob[4..12],
            &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn decode_absence_de_valeur_vaut_active() {
        assert!(decode_startup_state(&[]));
    }

    #[test]
    fn decode_blob_trop_court_vaut_active() {
        assert!(decode_startup_state(&[0x03, 0x00, 0x00]));
    }

    #[test]
    fn decode_0x03_vaut_desactive() {
        let blob = encode_startup_state(false, 42);
        assert!(!decode_startup_state(&blob));
    }

    #[test]
    fn decode_0x02_vaut_active() {
        let blob = encode_startup_state(true, 0);
        assert!(decode_startup_state(&blob));
    }

    #[test]
    fn decode_0x06_ecrit_par_un_autre_outil_vaut_active() {
        let mut blob = [0u8; 12];
        blob[0] = 0x06;
        assert!(decode_startup_state(&blob));
    }

    #[test]
    fn decode_0x07_ecrit_par_un_autre_outil_vaut_desactive() {
        // Windows et le Gestionnaire des tâches encodent l'état dans le bit 0 :
        // 0x02/0x06/0x0A sont activés, 0x03/0x07/0x0B désactivés.
        for premier in [0x03u8, 0x07, 0x0B] {
            let mut blob = [0u8; 12];
            blob[0] = premier;
            assert!(
                !decode_startup_state(&blob),
                "0x{premier:02X} doit être lu comme désactivé"
            );
        }
        for premier in [0x02u8, 0x06, 0x0A] {
            let mut blob = [0u8; 12];
            blob[0] = premier;
            assert!(
                decode_startup_state(&blob),
                "0x{premier:02X} doit être lu comme activé"
            );
        }
    }

    #[test]
    fn encode_puis_decode_est_une_identite() {
        assert!(decode_startup_state(&encode_startup_state(true, 0)));
        assert!(!decode_startup_state(&encode_startup_state(
            false,
            now_filetime()
        )));
    }

    #[test]
    fn identifiants_aller_retour() {
        assert_eq!(entry_id(StartupSource::Run, "OneDrive"), "run:OneDrive");
        assert_eq!(
            entry_id(StartupSource::RunOnce, "Patch"),
            "run-once:Patch"
        );
        assert_eq!(
            entry_id(StartupSource::Folder, "Notes.lnk"),
            "folder:Notes.lnk"
        );
        assert_eq!(
            parse_entry_id("run:OneDrive").unwrap(),
            (StartupSource::Run, "OneDrive".to_string())
        );
        assert_eq!(
            parse_entry_id("folder:Mon App.lnk").unwrap(),
            (StartupSource::Folder, "Mon App.lnk".to_string())
        );
    }

    #[test]
    fn parse_dun_identifiant_invalide_echoue() {
        assert!(matches!(
            parse_entry_id("bidon:X").unwrap_err(),
            StartupError::BadId(_)
        ));
        assert!(matches!(
            parse_entry_id("run").unwrap_err(),
            StartupError::BadId(_)
        ));
    }

    #[test]
    fn ecriture_puis_lecture_dun_blob_dans_hkcu() {
        let cle = CleClaireDeTest::new("approved");
        let blob = encode_startup_state(false, 0x1122_3344_5566_7788);

        assert_eq!(read_hkcu_binary(&cle.0, "MonApp").unwrap(), None);

        write_hkcu_binary(&cle.0, "MonApp", &blob).unwrap();
        let relu = read_hkcu_binary(&cle.0, "MonApp").unwrap().unwrap();
        assert_eq!(relu, blob.to_vec());
        assert!(!decode_startup_state(&relu));

        write_hkcu_binary(&cle.0, "MonApp", &encode_startup_state(true, 0)).unwrap();
        let relu = read_hkcu_binary(&cle.0, "MonApp").unwrap().unwrap();
        assert!(decode_startup_state(&relu));
    }

    #[test]
    fn lecture_dune_cle_absente_rend_none() {
        let absent = r"Software\wincleaner-test\jamais-cree";
        assert_eq!(read_hkcu_binary(absent, "X").unwrap(), None);
    }

    #[test]
    fn desactiver_une_entree_run_once_est_refuse() {
        let err = set_startup_enabled("run-once:Patch", false).unwrap_err();
        assert!(matches!(err, StartupError::ReadOnlySource));
    }

    /// Test couplé à l'environnement : il lit les vraies clés Run/RunOnce et
    /// le vrai dossier Démarrage de l'utilisateur courant (lecture seule, rien
    /// n'est modifié). La liste dépend donc du poste : les assertions ne
    /// portent que sur des invariants de forme, jamais sur un contenu attendu.
    #[test]
    fn lister_le_demarrage_ne_panique_pas() {
        let entries = list_startup().unwrap();
        for e in &entries {
            assert!(!e.id.is_empty());
            assert!(!e.name.is_empty());
            assert!(parse_entry_id(&e.id).is_ok());
        }
    }
}
