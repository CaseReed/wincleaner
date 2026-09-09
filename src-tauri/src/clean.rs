use crate::rules::{system_env, EnvLookup, Risk, Rule, RuleError, RuleKind};
use crate::scan::{query_recycle_bin, scan_rule_with_api, RecycleQuery};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanMode {
    Trash,
    Permanent,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CleanReport {
    pub freed_bytes: u64,
    pub deleted: u64,
    pub skipped: Vec<SkippedItem>,
}

impl CleanReport {
    pub fn merge(&mut self, other: CleanReport) {
        self.freed_bytes += other.freed_bytes;
        self.deleted += other.deleted;
        self.skipped.extend(other.skipped);
    }
}

/// Vidage de la corbeille, injecté pour rester testable.
pub type RecycleEmpty<'a> = &'a dyn Fn() -> Result<(), String>;

/// Résout `Auto` en fonction du risque de la règle. Ne renvoie jamais `Auto`.
pub fn effective_mode(mode: CleanMode, risk: Risk) -> CleanMode {
    match mode {
        CleanMode::Auto => match risk {
            Risk::Low => CleanMode::Permanent,
            Risk::Medium => CleanMode::Trash,
        },
        other => other,
    }
}

pub fn clean_rule_with_api(
    rule: &Rule,
    mode: CleanMode,
    lookup: EnvLookup,
    recycle_query: RecycleQuery,
    recycle_empty: RecycleEmpty,
) -> Result<CleanReport, RuleError> {
    // Re-scan interne juste avant suppression : le front n'a jamais envoyé
    // de chemin, et l'état du disque a pu changer depuis l'analyse.
    let scan = scan_rule_with_api(rule, lookup, recycle_query)?;

    if rule.kind == RuleKind::RecycleBin {
        return Ok(match recycle_empty() {
            Ok(()) => CleanReport {
                freed_bytes: scan.total_bytes,
                deleted: scan.file_count,
                skipped: Vec::new(),
            },
            Err(reason) => CleanReport {
                freed_bytes: 0,
                deleted: 0,
                skipped: vec![SkippedItem {
                    path: rule.label.clone(),
                    reason,
                }],
            },
        });
    }

    let target = effective_mode(mode, rule.risk);
    let mut report = CleanReport::default();

    for path in &scan.paths {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let outcome = match target {
            CleanMode::Permanent => std::fs::remove_file(path).map_err(|e| e.to_string()),
            CleanMode::Trash => trash::delete(path).map_err(|e| e.to_string()),
            CleanMode::Auto => unreachable!("effective_mode ne renvoie jamais Auto"),
        };
        match outcome {
            Ok(()) => {
                report.deleted += 1;
                report.freed_bytes += size;
            }
            Err(reason) => report.skipped.push(SkippedItem {
                path: path.clone(),
                reason,
            }),
        }
    }

    Ok(report)
}

pub fn clean_rule_with(
    rule: &Rule,
    mode: CleanMode,
    lookup: EnvLookup,
) -> Result<CleanReport, RuleError> {
    clean_rule_with_api(rule, mode, lookup, &query_recycle_bin, &empty_recycle_bin)
}

pub fn clean_rule(rule: &Rule, mode: CleanMode) -> Result<CleanReport, RuleError> {
    clean_rule_with(rule, mode, &system_env)
}

/// Vide la corbeille de tous les volumes, sans confirmation, sans barre de
/// progression et sans son. Irréversible : n'est appelée que par la règle
/// `windows.recycle-bin`.
pub fn empty_recycle_bin() -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    };

    let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), flags) }
        .map_err(|e| format!("SHEmptyRecycleBinW a échoué : {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Risk, Rule, RuleKind};
    use std::fs;
    use std::fs::OpenOptions;
    use std::path::Path;
    use tempfile::TempDir;

    fn faux_profil() -> TempDir {
        let dir = TempDir::new().unwrap();
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        fs::create_dir_all(temp.join("sub")).unwrap();
        fs::write(temp.join("a.txt"), b"aaa").unwrap();
        fs::write(temp.join("sub").join("b.txt"), b"bbbbb").unwrap();
        dir
    }

    fn lookup_for(root: &Path) -> impl Fn(&str) -> Option<String> + '_ {
        let root = root.to_string_lossy().to_string();
        move |name: &str| match name {
            "USERPROFILE" => Some(root.clone()),
            "TEMP" => Some(format!(r"{root}\AppData\Local\Temp")),
            "LOCALAPPDATA" => Some(format!(r"{root}\AppData\Local")),
            "APPDATA" => Some(format!(r"{root}\AppData\Roaming")),
            _ => None,
        }
    }

    fn regle_temp(risk: Risk) -> Rule {
        Rule {
            id: "windows.temp".into(),
            category: "Système".into(),
            label: "Fichiers temporaires".into(),
            paths: vec![r"%TEMP%\**\*".into()],
            exclude: vec![],
            risk,
            kind: RuleKind::Files,
        }
    }

    fn regle_corbeille() -> Rule {
        Rule {
            id: "windows.recycle-bin".into(),
            category: "Système".into(),
            label: "Corbeille".into(),
            paths: vec![],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::RecycleBin,
        }
    }

    fn recycle_interdit_query() -> Result<(u64, u64), String> {
        panic!("l'API corbeille ne doit pas être appelée pour une règle « files »");
    }

    fn recycle_interdit_empty() -> Result<(), String> {
        panic!("l'API corbeille ne doit pas être appelée pour une règle « files »");
    }

    #[test]
    fn auto_devient_permanent_pour_un_risque_faible() {
        assert_eq!(
            effective_mode(CleanMode::Auto, Risk::Low),
            CleanMode::Permanent
        );
    }

    #[test]
    fn auto_devient_corbeille_pour_un_risque_moyen() {
        assert_eq!(
            effective_mode(CleanMode::Auto, Risk::Medium),
            CleanMode::Trash
        );
    }

    #[test]
    fn un_mode_explicite_nest_pas_reinterprete() {
        assert_eq!(
            effective_mode(CleanMode::Trash, Risk::Low),
            CleanMode::Trash
        );
        assert_eq!(
            effective_mode(CleanMode::Permanent, Risk::Medium),
            CleanMode::Permanent
        );
    }

    #[test]
    fn suppression_definitive_efface_les_fichiers() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(Risk::Low);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &recycle_interdit_query,
            &recycle_interdit_empty,
        )
        .unwrap();
        assert_eq!(report.deleted, 2);
        assert_eq!(report.freed_bytes, 8);
        assert!(report.skipped.is_empty());
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        assert!(!temp.join("a.txt").exists());
        assert!(!temp.join("sub").join("b.txt").exists());
    }

    #[test]
    fn suppression_corbeille_efface_les_fichiers_du_repertoire() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(Risk::Medium);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Trash,
            &lookup,
            &recycle_interdit_query,
            &recycle_interdit_empty,
        )
        .unwrap();
        assert_eq!(report.deleted, 2);
        assert_eq!(report.freed_bytes, 8);
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        assert!(!temp.join("a.txt").exists());
    }

    #[test]
    fn un_fichier_verrouille_finit_dans_skipped() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        let verrou = temp.join("a.txt");
        // Ouverture en écriture avec partage refusé : Windows renvoie
        // ERROR_SHARING_VIOLATION à toute tentative de suppression.
        let _handle = {
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                OpenOptions::new()
                    .write(true)
                    .share_mode(0)
                    .open(&verrou)
                    .unwrap()
            }
            #[cfg(not(windows))]
            {
                OpenOptions::new().write(true).open(&verrou).unwrap()
            }
        };

        let rule = regle_temp(Risk::Low);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &recycle_interdit_query,
            &recycle_interdit_empty,
        )
        .unwrap();

        assert_eq!(report.deleted, 1);
        assert_eq!(report.freed_bytes, 5);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].path.ends_with("a.txt"));
        assert!(!report.skipped[0].reason.is_empty());
        assert!(verrou.exists());
    }

    #[test]
    fn nettoyer_un_repertoire_vide_ne_fait_rien() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(Risk::Low);
        let report = clean_rule_with_api(
            &rule,
            CleanMode::Permanent,
            &lookup,
            &recycle_interdit_query,
            &recycle_interdit_empty,
        )
        .unwrap();
        assert_eq!(report.deleted, 0);
        assert_eq!(report.freed_bytes, 0);
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn merge_additionne_deux_rapports() {
        let mut a = CleanReport {
            freed_bytes: 10,
            deleted: 2,
            skipped: vec![SkippedItem {
                path: "x".into(),
                reason: "y".into(),
            }],
        };
        a.merge(CleanReport {
            freed_bytes: 5,
            deleted: 1,
            skipped: vec![SkippedItem {
                path: "z".into(),
                reason: "w".into(),
            }],
        });
        assert_eq!(a.freed_bytes, 15);
        assert_eq!(a.deleted, 3);
        assert_eq!(a.skipped.len(), 2);
    }

    #[test]
    fn la_regle_corbeille_appelle_lapi_corbeille() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let query = || Ok((4u64, 4096u64));
        let empty = || Ok(());
        let report = clean_rule_with_api(
            &regle_corbeille(),
            CleanMode::Auto,
            &lookup,
            &query,
            &empty,
        )
        .unwrap();
        assert_eq!(report.deleted, 4);
        assert_eq!(report.freed_bytes, 4096);
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn un_echec_de_lapi_corbeille_finit_dans_skipped() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let query = || Ok((4u64, 4096u64));
        let empty = || Err("accès refusé".to_string());
        let report =
            clean_rule_with_api(&regle_corbeille(), CleanMode::Auto, &lookup, &query, &empty)
                .unwrap();
        assert_eq!(report.deleted, 0);
        assert_eq!(report.freed_bytes, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, "accès refusé");
    }
}
