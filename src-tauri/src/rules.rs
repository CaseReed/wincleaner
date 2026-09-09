use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// Contenu de `src-tauri/rules.toml`, embarqué dans le binaire.
pub const RULES_TOML: &str = include_str!("../rules.toml");

/// Liste blanche exhaustive des variables d'environnement utilisables
/// dans un chemin de règle.
pub const ALLOWED_VARS: [&str; 4] = ["TEMP", "LOCALAPPDATA", "APPDATA", "USERPROFILE"];

/// Fonction de résolution d'une variable d'environnement. Injectée pour
/// que les tests n'aient jamais besoin du vrai profil utilisateur.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuleKind {
    #[default]
    Files,
    RecycleBin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub category: String,
    pub label: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub risk: Risk,
    #[serde(default)]
    pub kind: RuleKind,
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    rule: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    /// Le chemin ne commence pas par `%VAR%\`.
    NotVarPrefixed(String),
    /// Variable absente de la liste blanche.
    UnknownVar(String),
    /// Variable de la liste blanche mais non définie dans l'environnement.
    MissingVar(String),
    /// Un segment `..` a été trouvé.
    ParentSegment(String),
    /// Le chemin résolu sort de `%USERPROFILE%`.
    OutsideProfile(String),
    /// Deux règles portent le même `id`.
    DuplicateId(String),
    /// Une règle `kind = "files"` n'a aucun chemin.
    EmptyPaths(String),
    /// Erreur de syntaxe ou de typage TOML.
    Toml(String),
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::NotVarPrefixed(p) => write!(
                f,
                "le chemin « {p} » doit commencer par une variable, par exemple %TEMP%\\"
            ),
            RuleError::UnknownVar(v) => write!(
                f,
                "la variable %{v}% n'est pas autorisée (autorisées : TEMP, LOCALAPPDATA, APPDATA, USERPROFILE)"
            ),
            RuleError::MissingVar(v) => {
                write!(f, "la variable d'environnement %{v}% n'est pas définie")
            }
            RuleError::ParentSegment(p) => {
                write!(f, "le chemin « {p} » contient un segment « .. »")
            }
            RuleError::OutsideProfile(p) => {
                write!(f, "le chemin « {p} » sort du profil utilisateur")
            }
            RuleError::DuplicateId(id) => write!(f, "l'identifiant de règle « {id} » est dupliqué"),
            RuleError::EmptyPaths(id) => write!(
                f,
                "la règle « {id} » est de type « files » mais ne déclare aucun chemin"
            ),
            RuleError::Toml(m) => write!(f, "rules.toml est invalide : {m}"),
        }
    }
}

impl std::error::Error for RuleError {}

/// Résolution réelle, adossée à l'environnement du processus.
pub fn system_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Remplace la variable `%VAR%` de tête. Une seule variable est acceptée,
/// et uniquement en tête : un chemin de règle est toujours de la forme
/// `%VAR%\reste`.
pub fn expand_env_with(raw: &str, lookup: EnvLookup) -> Result<String, RuleError> {
    if !raw.starts_with('%') {
        return Err(RuleError::NotVarPrefixed(raw.to_string()));
    }
    let end = raw[1..]
        .find('%')
        .ok_or_else(|| RuleError::NotVarPrefixed(raw.to_string()))?
        + 1;
    let name = &raw[1..end];
    let rest = &raw[end + 1..];
    if !ALLOWED_VARS.contains(&name) {
        return Err(RuleError::UnknownVar(name.to_string()));
    }
    if !(rest.starts_with('\\') || rest.starts_with('/') || rest.is_empty()) {
        return Err(RuleError::NotVarPrefixed(raw.to_string()));
    }
    let value = lookup(name).ok_or_else(|| RuleError::MissingVar(name.to_string()))?;
    let value = value.trim_end_matches(['\\', '/']).to_string();
    Ok(format!("{value}{rest}"))
}

pub fn expand_env(raw: &str) -> Result<String, RuleError> {
    expand_env_with(raw, &system_env)
}

/// Met le chemin sous forme Windows (`\`) et refuse tout segment `..`.
pub fn normalize(raw: &str) -> Result<String, RuleError> {
    let win = raw.replace('/', "\\");
    if win.split('\\').any(|seg| seg == "..") {
        return Err(RuleError::ParentSegment(raw.to_string()));
    }
    Ok(win)
}

fn under_profile(path: &str, profile: &str) -> bool {
    let p = path.to_lowercase();
    let root = profile.trim_end_matches('\\').to_lowercase();
    p == root || p.starts_with(&format!("{root}\\"))
}

fn resolve_one(raw: &str, lookup: EnvLookup) -> Result<String, RuleError> {
    let expanded = expand_env_with(raw, lookup)?;
    let normalized = normalize(&expanded)?;
    let profile = lookup("USERPROFILE")
        .ok_or_else(|| RuleError::MissingVar("USERPROFILE".to_string()))?;
    let profile = normalize(&profile)?;
    if !under_profile(&normalized, &profile) {
        return Err(RuleError::OutsideProfile(raw.to_string()));
    }
    Ok(normalized)
}

pub fn resolved_paths_with(rule: &Rule, lookup: EnvLookup) -> Result<Vec<String>, RuleError> {
    rule.paths.iter().map(|p| resolve_one(p, lookup)).collect()
}

pub fn resolved_excludes_with(rule: &Rule, lookup: EnvLookup) -> Result<Vec<String>, RuleError> {
    rule.exclude.iter().map(|p| resolve_one(p, lookup)).collect()
}

pub fn load_rules_with(src: &str, lookup: EnvLookup) -> Result<Vec<Rule>, RuleError> {
    let parsed: RuleFile =
        toml::from_str(src).map_err(|e| RuleError::Toml(e.to_string()))?;
    let mut seen: HashSet<String> = HashSet::new();
    for rule in &parsed.rule {
        if !seen.insert(rule.id.clone()) {
            return Err(RuleError::DuplicateId(rule.id.clone()));
        }
        if rule.kind == RuleKind::Files && rule.paths.is_empty() {
            return Err(RuleError::EmptyPaths(rule.id.clone()));
        }
        resolved_paths_with(rule, lookup)?;
        resolved_excludes_with(rule, lookup)?;
    }
    Ok(parsed.rule)
}

pub fn load_rules(src: &str) -> Result<Vec<Rule>, RuleError> {
    load_rules_with(src, &system_env)
}

/// Charge les règles embarquées avec le vrai environnement. Une erreur ici
/// est bloquante : l'application doit refuser de démarrer.
pub fn embedded_rules() -> Result<Vec<Rule>, RuleError> {
    load_rules(RULES_TOML)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_env(name: &str) -> Option<String> {
        match name {
            "USERPROFILE" => Some(r"C:\Users\Test".to_string()),
            "TEMP" => Some(r"C:\Users\Test\AppData\Local\Temp".to_string()),
            "LOCALAPPDATA" => Some(r"C:\Users\Test\AppData\Local".to_string()),
            "APPDATA" => Some(r"C:\Users\Test\AppData\Roaming".to_string()),
            _ => None,
        }
    }

    fn toml_one(body: &str) -> String {
        format!("[[rule]]\n{}\n", body)
    }

    #[test]
    fn expand_remplace_la_variable_de_tete() {
        let got = expand_env_with(r"%TEMP%\a\b", &fake_env).unwrap();
        assert_eq!(got, r"C:\Users\Test\AppData\Local\Temp\a\b");
    }

    #[test]
    fn expand_refuse_une_variable_hors_liste_blanche() {
        let err = expand_env_with(r"%WINDIR%\a", &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::UnknownVar(ref v) if v == "WINDIR"));
    }

    #[test]
    fn expand_refuse_un_chemin_sans_variable_de_tete() {
        let err = expand_env_with(r"C:\Windows\Temp\*", &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::NotVarPrefixed(_)));
    }

    #[test]
    fn normalize_convertit_les_slashs_et_refuse_les_segments_parents() {
        assert_eq!(normalize("C:/a/b").unwrap(), r"C:\a\b");
        let err = normalize(r"C:\a\..\b").unwrap_err();
        assert!(matches!(err, RuleError::ParentSegment(_)));
    }

    #[test]
    fn charge_une_regle_valide() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let rules = load_rules_with(&src, &fake_env).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "x.y");
        assert_eq!(rules[0].risk, Risk::Low);
        assert_eq!(rules[0].kind, RuleKind::Files);
    }

    #[test]
    fn refuse_un_chemin_hors_du_profil() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%USERPROFILE%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let hors_profil = |name: &str| match name {
            "USERPROFILE" => Some(r"C:\Users\Autre".to_string()),
            _ => fake_env(name),
        };
        // %TEMP% pointe sur C:\Users\Test alors que le profil est C:\Users\Autre
        let src2 = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        assert!(load_rules_with(&src, &hors_profil).is_ok());
        let err = load_rules_with(&src2, &hors_profil).unwrap_err();
        assert!(matches!(err, RuleError::OutsideProfile(_)));
    }

    #[test]
    fn refuse_une_variable_inconnue_dans_une_regle() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%SYSTEMROOT%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let err = load_rules_with(&src, &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::UnknownVar(ref v) if v == "SYSTEMROOT"));
    }

    #[test]
    fn refuse_un_segment_parent_dans_une_regle() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%TEMP%\\..\\..\\Windows\\*"]
exclude = []
risk = "low""#,
        );
        let err = load_rules_with(&src, &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::ParentSegment(_)));
    }

    #[test]
    fn refuse_un_id_duplique() {
        let src = format!(
            "{}{}",
            toml_one(
                r#"id = "x.y"
category = "Système"
label = "A"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low""#
            ),
            toml_one(
                r#"id = "x.y"
category = "Système"
label = "B"
paths = ["%TEMP%\\*"]
exclude = []
risk = "low""#
            )
        );
        let err = load_rules_with(&src, &fake_env).unwrap_err();
        assert!(matches!(err, RuleError::DuplicateId(ref v) if v == "x.y"));
    }

    #[test]
    fn refuse_un_risque_inconnu() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%TEMP%\\*"]
exclude = []
risk = "high""#,
        );
        assert!(matches!(
            load_rules_with(&src, &fake_env).unwrap_err(),
            RuleError::Toml(_)
        ));
    }

    #[test]
    fn la_regle_corbeille_na_pas_de_chemin() {
        let src = toml_one(
            r#"id = "windows.recycle-bin"
category = "Système"
label = "Corbeille"
kind = "recycle-bin"
paths = []
exclude = []
risk = "low""#,
        );
        let rules = load_rules_with(&src, &fake_env).unwrap();
        assert_eq!(rules[0].kind, RuleKind::RecycleBin);
        assert!(rules[0].paths.is_empty());
    }

    #[test]
    fn refuse_une_regle_files_sans_chemin() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = []
exclude = []
risk = "low""#,
        );
        assert!(matches!(
            load_rules_with(&src, &fake_env).unwrap_err(),
            RuleError::EmptyPaths(_)
        ));
    }

    #[test]
    fn le_rules_toml_embarque_est_valide() {
        let rules = load_rules_with(RULES_TOML, &fake_env).unwrap();
        assert_eq!(rules.len(), 8);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "windows.temp",
                "windows.recycle-bin",
                "windows.thumbnails",
                "windows.explorer-recent",
                "windows.user-logs",
                "edge.cache",
                "chrome.cache",
                "firefox.cache",
            ]
        );
    }

    #[test]
    fn resolved_paths_rend_les_chemins_absolus() {
        let rule = Rule {
            id: "x.y".into(),
            category: "Système".into(),
            label: "Test".into(),
            paths: vec![r"%TEMP%\**\*".into()],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::Files,
        };
        let got = resolved_paths_with(&rule, &fake_env).unwrap();
        assert_eq!(got, vec![r"C:\Users\Test\AppData\Local\Temp\**\*"]);
    }
}
