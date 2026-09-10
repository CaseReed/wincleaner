use crate::rules::{profile_canon_with, system_env, EnvLookup, Risk, Rule, RuleError, RuleKind};
use crate::scan::{est_point_danalyse, query_recycle_bin, scan_rule_with_api, RecycleQuery};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// Retire le préfixe verbatim que `canonicalize` ajoute. `IFileOperation`,
/// derrière `trash::delete`, n'accepte pas un chemin `\?\`.
fn sans_prefixe_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\?\UNC\") {
        Some(reste) => PathBuf::from(format!(r"\{reste}")),
        None => PathBuf::from(s.strip_prefix(r"\?\").unwrap_or(&s)),
    }
}

/// Dernière vérification avant de supprimer : le re-scan a beau être
/// immédiat, un processus tournant sous le même compte peut remplacer un
/// nom entre le `metadata()` du parcours et l'appel de suppression. On exige
/// donc, sur le chemin lui-même et non sur ce qu'il pointe, un fichier
/// régulier dont l'emplacement réel reste sous le profil.
///
/// Rend le chemin à supprimer et sa taille.
fn chemin_supprimable(path: &str, profile_canon: &Path) -> Result<(PathBuf, u64), String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if est_point_danalyse(&md) {
        return Err("le chemin est devenu un point d'analyse".to_string());
    }
    if !md.is_file() {
        return Err("le chemin n'est plus un fichier régulier".to_string());
    }
    let reel = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !reel.starts_with(profile_canon) {
        return Err("le chemin sort du profil utilisateur au moment de la suppression".to_string());
    }
    Ok((sans_prefixe_verbatim(&reel), md.len()))
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
    let profile_canon = profile_canon_with(lookup)?;
    let mut report = CleanReport::default();

    for path in &scan.paths {
        let (reel, size) = match chemin_supprimable(path, &profile_canon) {
            Ok(v) => v,
            Err(reason) => {
                report.skipped.push(SkippedItem {
                    path: path.clone(),
                    reason,
                });
                continue;
            }
        };
        let outcome = match target {
            CleanMode::Permanent => std::fs::remove_file(&reel).map_err(|e| e.to_string()),
            CleanMode::Trash => trash::delete(&reel).map_err(|e| e.to_string()),
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

pub fn clean_rule(rule: &Rule, mode: CleanMode) -> Result<CleanReport, RuleError> {
    clean_rule_with_api(
        rule,
        mode,
        &system_env,
        &query_recycle_bin,
        &empty_recycle_bin,
    )
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
            default_checked: true,
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
            default_checked: false,
        }
    }

    fn recycle_interdit_query() -> Result<(u64, u64), String> {
        panic!("l'API corbeille ne doit pas être appelée pour une règle « files »");
    }

    fn recycle_interdit_empty() -> Result<(), String> {
        panic!("l'API corbeille ne doit pas être appelée pour une règle « files »");
    }

    fn jonction(lien: &Path, cible: &Path) {
        let out = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(lien)
            .arg(cible)
            .output()
            .expect("mklink n'a pas pu être lancé");
        assert!(
            out.status.success(),
            "mklink /J a échoué : {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn un_fichier_reel_sous_le_profil_est_supprimable() {
        let dir = TempDir::new().unwrap();
        let profil = std::fs::canonicalize(dir.path()).unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, b"aaa").unwrap();
        let (_, taille) =
            chemin_supprimable(&f.to_string_lossy(), &profil).expect("devrait être supprimable");
        assert_eq!(taille, 3);
    }

    #[test]
    fn un_chemin_devenu_repertoire_entre_le_scan_et_la_suppression_est_ignore() {
        let dir = TempDir::new().unwrap();
        let profil = std::fs::canonicalize(dir.path()).unwrap();
        let d = dir.path().join("a.txt");
        fs::create_dir(&d).unwrap();
        let err = chemin_supprimable(&d.to_string_lossy(), &profil).unwrap_err();
        assert!(err.contains("fichier régulier"), "raison = {err}");
    }

    #[test]
    fn un_chemin_devenu_jonction_entre_le_scan_et_la_suppression_est_ignore() {
        let base = TempDir::new().unwrap();
        let profil_dir = base.path().join("profil");
        let dehors = base.path().join("dehors");
        fs::create_dir_all(&profil_dir).unwrap();
        fs::create_dir_all(&dehors).unwrap();
        fs::write(dehors.join("precieux.txt"), b"precieux").unwrap();
        let profil = std::fs::canonicalize(&profil_dir).unwrap();
        // Le nom qui avait été analysé comme un fichier est devenu une jonction.
        let piege = profil_dir.join("a.txt");
        jonction(&piege, &dehors);

        let err = chemin_supprimable(&piege.to_string_lossy(), &profil).unwrap_err();
        assert!(err.contains("point d'analyse"), "raison = {err}");
        assert!(dehors.join("precieux.txt").exists());
    }

    #[test]
    fn un_fichier_hors_du_profil_nest_pas_supprimable() {
        let base = TempDir::new().unwrap();
        let profil_dir = base.path().join("profil");
        let dehors = base.path().join("dehors");
        fs::create_dir_all(&profil_dir).unwrap();
        fs::create_dir_all(&dehors).unwrap();
        let f = dehors.join("precieux.txt");
        fs::write(&f, b"precieux").unwrap();
        let profil = std::fs::canonicalize(&profil_dir).unwrap();

        let err = chemin_supprimable(&f.to_string_lossy(), &profil).unwrap_err();
        assert!(err.contains("sort du profil"), "raison = {err}");
        assert!(f.exists());
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
