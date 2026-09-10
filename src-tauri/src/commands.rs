use crate::clean::{clean_rule, CleanMode, CleanReport, SkippedItem};
use crate::rules::{embedded_rules, Risk, Rule, RuleKind};
use crate::scan::{scan_rule, ScanResult};
use crate::startup::StartupEntry;
use serde::{Deserialize, Serialize};
use sysinfo::System;

/// Processus considérés comme « navigateur ouvert » pour l'avertissement.
pub const BROWSER_PROCESSES: [&str; 3] = ["msedge.exe", "chrome.exe", "firefox.exe"];

/// Ce que le front reçoit pour construire la liste des règles. Ne contient
/// jamais de chemin : le front n'a pas à connaître ce qui sera supprimé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSummary {
    pub id: String,
    pub category: String,
    pub label: String,
    pub risk: Risk,
    pub kind: RuleKind,
}

fn find_rules(rule_ids: &[String]) -> Result<Vec<Rule>, String> {
    let all = embedded_rules().map_err(|e| e.to_string())?;
    rule_ids
        .iter()
        .map(|id| {
            all.iter()
                .find(|r| &r.id == id)
                .cloned()
                .ok_or_else(|| format!("règle inconnue : « {id} »"))
        })
        .collect()
}

pub fn running_browsers_from(process_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in process_names {
        let lower = name.to_lowercase();
        if BROWSER_PROCESSES.contains(&lower.as_str()) && !out.contains(&lower) {
            out.push(lower);
        }
    }
    out
}

#[tauri::command]
pub fn list_rules() -> Result<Vec<RuleSummary>, String> {
    let rules = embedded_rules().map_err(|e| e.to_string())?;
    Ok(rules
        .into_iter()
        .map(|r| RuleSummary {
            id: r.id,
            category: r.category,
            label: r.label,
            risk: r.risk,
            kind: r.kind,
        })
        .collect())
}

#[tauri::command]
pub fn scan(rule_ids: Vec<String>) -> Result<Vec<ScanResult>, String> {
    let rules = find_rules(&rule_ids)?;
    rules
        .iter()
        .map(|r| scan_rule(r).map_err(|e| e.to_string()))
        .collect()
}

/// Applique `run` à chaque règle et fusionne les rapports. L'échec d'une
/// règle ne jette jamais ce qui a déjà été nettoyé : il devient une entrée
/// `skipped` portant l'identifiant de la règle.
fn clean_all(
    rules: &[Rule],
    mut run: impl FnMut(&Rule) -> Result<CleanReport, String>,
) -> CleanReport {
    let mut report = CleanReport::default();
    for rule in rules {
        match run(rule) {
            Ok(partiel) => report.merge(partiel),
            Err(reason) => report.skipped.push(SkippedItem {
                path: rule.id.clone(),
                reason,
            }),
        }
    }
    report
}

#[tauri::command]
pub fn clean(rule_ids: Vec<String>, mode: CleanMode) -> Result<CleanReport, String> {
    let rules = find_rules(&rule_ids)?;
    Ok(clean_all(&rules, |rule| {
        clean_rule(rule, mode).map_err(|e| e.to_string())
    }))
}

#[tauri::command]
pub fn running_browsers() -> Vec<String> {
    let mut system = System::new_all();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let names: Vec<String> = system
        .processes()
        .values()
        .map(|p| p.name().to_string_lossy().to_string())
        .collect();
    running_browsers_from(&names)
}

#[tauri::command]
pub fn list_startup() -> Result<Vec<StartupEntry>, String> {
    crate::startup::list_startup().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_startup_enabled(id: String, enabled: bool) -> Result<(), String> {
    crate::startup::set_startup_enabled(&id, enabled).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ne_retient_que_les_navigateurs_cibles() {
        let noms: Vec<String> = [
            "explorer.exe",
            "msedge.exe",
            "MSEDGE.EXE",
            "chrome.exe",
            "code.exe",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut got = running_browsers_from(&noms);
        got.sort();
        assert_eq!(got, vec!["chrome.exe", "msedge.exe"]);
    }

    #[test]
    fn deduplique_les_processus_multiples() {
        let noms: Vec<String> = ["firefox.exe", "firefox.exe", "firefox.exe"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(running_browsers_from(&noms), vec!["firefox.exe"]);
    }

    #[test]
    fn aucun_navigateur_rend_une_liste_vide() {
        let noms: Vec<String> = ["explorer.exe".to_string()].to_vec();
        assert!(running_browsers_from(&noms).is_empty());
    }

    #[test]
    fn le_resume_des_regles_reprend_les_huit_regles_embarquees() {
        let resumes = list_rules().unwrap();
        assert_eq!(resumes.len(), 8);
        assert_eq!(resumes[0].id, "windows.temp");
        assert_eq!(resumes[1].kind, crate::rules::RuleKind::RecycleBin);
    }

    #[test]
    fn scanner_un_identifiant_inconnu_est_une_erreur() {
        let err = scan(vec!["inexistant".to_string()]).unwrap_err();
        assert!(err.contains("inexistant"));
    }

    fn regle(id: &str) -> Rule {
        Rule {
            id: id.into(),
            category: "Système".into(),
            label: id.into(),
            paths: vec![r"%TEMP%\*".into()],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::Files,
        }
    }

    #[test]
    fn lechec_dune_regle_ne_jette_pas_le_rapport_des_precedentes() {
        let regles = [regle("a"), regle("b"), regle("c")];
        let report = clean_all(&regles, |r| {
            if r.id == "b" {
                Err("accès refusé".to_string())
            } else {
                Ok(CleanReport {
                    freed_bytes: 10,
                    deleted: 1,
                    skipped: Vec::new(),
                })
            }
        });
        assert_eq!(report.deleted, 2);
        assert_eq!(report.freed_bytes, 20);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].path, "b");
        assert_eq!(report.skipped[0].reason, "accès refusé");
    }

    #[test]
    fn nettoyer_un_identifiant_inconnu_est_une_erreur() {
        let err = clean(vec!["inexistant".to_string()], crate::clean::CleanMode::Auto)
            .unwrap_err();
        assert!(err.contains("inexistant"));
    }
}
