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
    /// Case cochée au premier lancement (cf. `rules.toml`).
    pub default_checked: bool,
    /// Renseigné quand la règle ne s'applique pas sur cette machine. Le front
    /// grise la ligne et affiche ce motif ; la règle n'est ni analysée ni
    /// nettoyée, même si son identifiant était envoyé.
    pub unavailable_reason: Option<String>,
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
            default_checked: r.default_checked && r.unavailable_reason.is_none(),
            unavailable_reason: r.unavailable_reason,
        })
        .collect())
}

/// Exécute un travail bloquant hors du fil principal. Une commande Tauri
/// synchrone s'exécute sur le fil principal et gèle la boucle d'évènements de
/// la webview le temps de son exécution : sur un profil chargé, un parcours de
/// disque de plusieurs secondes rendrait la fenêtre « ne répond pas ».
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("tâche interrompue : {e}"))?
}

fn scan_rules(rule_ids: &[String]) -> Result<Vec<ScanResult>, String> {
    let rules = find_rules(rule_ids)?;
    rules
        .iter()
        .map(|r| match &r.unavailable_reason {
            // La règle ne s'applique pas sur cette machine : rien à parcourir,
            // et surtout rien à supprimer. Comptée « ignorée », pas en erreur.
            Some(_) => Ok(ScanResult {
                rule_id: r.id.clone(),
                file_count: 0,
                total_bytes: 0,
                paths: Vec::new(),
                skipped: 1,
            }),
            None => scan_rule(r).map_err(|e| e.to_string()),
        })
        .collect()
}

#[tauri::command]
pub async fn scan(rule_ids: Vec<String>) -> Result<Vec<ScanResult>, String> {
    blocking(move || scan_rules(&rule_ids)).await
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

/// La corbeille est vidée EN PREMIER, quel que soit l'ordre de rules.toml ou
/// celui que le front envoie. Sinon, en mode « Corbeille », les règles
/// « files » y déposent leurs fichiers et la règle Corbeille les détruit
/// définitivement dans la même passe : le mode prudent ne protège plus rien.
/// `sort_by_key` est stable : l'ordre relatif des autres règles est conservé.
fn ordre_de_nettoyage(mut rules: Vec<Rule>) -> Vec<Rule> {
    rules.sort_by_key(|r| r.kind != RuleKind::RecycleBin);
    rules
}

fn clean_rules(rule_ids: &[String], mode: CleanMode) -> Result<CleanReport, String> {
    let rules = ordre_de_nettoyage(find_rules(rule_ids)?);
    Ok(clean_all(&rules, |rule| match &rule.unavailable_reason {
        Some(raison) => Err(raison.clone()),
        None => clean_rule(rule, mode).map_err(|e| e.to_string()),
    }))
}

#[tauri::command]
pub async fn clean(rule_ids: Vec<String>, mode: CleanMode) -> Result<CleanReport, String> {
    blocking(move || clean_rules(&rule_ids, mode)).await
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
pub async fn list_startup() -> Result<Vec<StartupEntry>, String> {
    blocking(|| crate::startup::list_startup().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn set_startup_enabled(id: String, enabled: bool) -> Result<(), String> {
    blocking(move || crate::startup::set_startup_enabled(&id, enabled).map_err(|e| e.to_string()))
        .await
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

    /// Le front construit ses cases à partir de ce champ : s'il ne traverse
    /// pas l'IPC, tout redevient coché par défaut.
    #[test]
    fn le_resume_transmet_la_case_par_defaut() {
        let resumes = list_rules().unwrap();
        let corbeille = resumes
            .iter()
            .find(|r| r.id == "windows.recycle-bin")
            .unwrap();
        assert!(!corbeille.default_checked);
        let temp = resumes.iter().find(|r| r.id == "windows.temp").unwrap();
        assert!(temp.default_checked);
    }

    /// Le point de l'item : une commande synchrone s'exécuterait sur le fil
    /// appelant — le fil principal en production, celui qui pompe les
    /// évènements de la fenêtre. `blocking` doit déporter le travail ailleurs.
    #[test]
    fn le_travail_bloquant_quitte_le_fil_appelant() {
        let appelant = std::thread::current().id();
        let dedans =
            tauri::async_runtime::block_on(blocking(|| Ok(std::thread::current().id()))).unwrap();
        assert_ne!(appelant, dedans);
    }

    /// Passe par la commande asynchrone, donc par `spawn_blocking` : vérifie
    /// que le travail déporté hors du fil principal rend bien son résultat.
    #[test]
    fn scanner_un_identifiant_inconnu_est_une_erreur() {
        let err = tauri::async_runtime::block_on(scan(vec!["inexistant".to_string()])).unwrap_err();
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
            default_checked: true,
            unavailable_reason: None,
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
            unavailable_reason: None,
        }
    }

    /// Une règle indisponible ne doit jamais atteindre le disque, même si le
    /// front envoie son identifiant.
    #[test]
    fn une_regle_indisponible_nest_ni_analysee_ni_nettoyee() {
        let mut regle = regle("x.y");
        regle.paths = vec![r"%TEMP%\**\*".into()];
        regle.unavailable_reason = Some("%TEMP% sort du profil".into());

        let rapport = clean_all(std::slice::from_ref(&regle), |r| {
            match &r.unavailable_reason {
                Some(raison) => Err(raison.clone()),
                None => panic!("une règle indisponible ne doit pas être nettoyée"),
            }
        });
        assert_eq!(rapport.deleted, 0);
        assert_eq!(rapport.skipped.len(), 1);
        assert_eq!(rapport.skipped[0].path, "x.y");
    }

    #[test]
    fn la_regle_corbeille_est_traitee_avant_les_regles_fichiers() {
        // Sinon, en mode « Corbeille », la règle 2 vide ce que la règle 1
        // vient d'y déposer : le mode prudent détruit définitivement.
        let regles = ordre_de_nettoyage(vec![regle("a"), regle_corbeille(), regle("b")]);
        let vus = std::cell::RefCell::new(Vec::new());
        clean_all(&regles, |r| {
            vus.borrow_mut().push(r.id.clone());
            Ok(CleanReport::default())
        });
        assert_eq!(*vus.borrow(), vec!["windows.recycle-bin", "a", "b"]);
    }

    #[test]
    fn lordre_des_regles_fichiers_est_conserve() {
        let regles = ordre_de_nettoyage(vec![regle("a"), regle("b"), regle("c")]);
        let ids: Vec<&str> = regles.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn nettoyer_un_identifiant_inconnu_est_une_erreur() {
        let err = tauri::async_runtime::block_on(clean(
            vec!["inexistant".to_string()],
            crate::clean::CleanMode::Auto,
        ))
        .unwrap_err();
        assert!(err.contains("inexistant"));
    }
}
