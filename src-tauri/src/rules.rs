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
    /// Un motif de chemin n'est pas un glob valide. Distinct de `Toml` :
    /// le fichier peut être syntaxiquement correct et le motif fautif.
    Glob { pattern: String, cause: String },
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
            RuleError::Glob { pattern, cause } => {
                write!(f, "glob « {pattern} » invalide : {cause}")
            }
        }
    }
}

impl std::error::Error for RuleError {}

/// Convertit un chemin Windows (potentiellement en forme courte 8.3, comme
/// le `%TEMP%` que Windows peut fournir quand le nom de compte contient un
/// espace) vers sa forme longue. Si le chemin n'existe pas ou que l'appel
/// échoue, l'entrée est renvoyée inchangée.
/// Découpe le tampon rendu par `GetLongPathNameW`. `None` si l'appel a échoué
/// (`written == 0`) ou si la longueur annoncée dépasse le tampon — le chemin a
/// pu s'allonger entre les deux appels, et découper hors bornes paniquerait,
/// ce qui avorte le processus (`panic = "abort"`).
fn long_path_from_buffer(buf: &[u16], written: u32) -> Option<String> {
    let written = written as usize;
    if written == 0 || written > buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..written]))
}

fn long_path(value: &str) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;

    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let src = PCWSTR(wide.as_ptr());
    let len = unsafe { GetLongPathNameW(src, None) };
    if len == 0 {
        return value.to_string();
    }
    let mut buf = vec![0u16; len as usize];
    let written = unsafe { GetLongPathNameW(src, Some(&mut buf)) };
    long_path_from_buffer(&buf, written).unwrap_or_else(|| value.to_string())
}

/// Résolution réelle, adossée à l'environnement du processus.
pub fn system_env(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    if ALLOWED_VARS.contains(&name) {
        Some(long_path(&value))
    } else {
        Some(value)
    }
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
    // Seule la valeur de la variable est échappée : le suffixe est écrit par
    // la règle et ses jokers doivent rester des jokers. `globset::escape`
    // enferme chaque métacaractère dans une classe à un caractère (`[*]`),
    // jamais derrière un antislash : l'échappement survit donc à la réécriture
    // de `\` en `/` faite plus loin dans la chaîne de traitement.
    let value = globset::escape(value.trim_end_matches(['\\', '/']));
    Ok(format!("{value}{rest}"))
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
    // Le profil passe par `expand_env_with` pour subir exactement le même
    // échappement que le chemin comparé.
    let profile = expand_env_with("%USERPROFILE%", lookup)?;
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
    fn expand_echappe_les_metacaracteres_de_la_valeur_mais_pas_le_suffixe() {
        let profil_crochets = |name: &str| match name {
            "USERPROFILE" | "TEMP" => Some(r"C:\Users\a[b]c".to_string()),
            _ => None,
        };
        let got = expand_env_with(r"%TEMP%\**\*", &profil_crochets).unwrap();
        // La valeur devient littérale, le `**\*` écrit par la règle reste un joker.
        assert_eq!(got, r"C:\Users\a[[]b[]]c\**\*");
    }

    #[test]
    fn expand_echappe_tous_les_metacaracteres_de_glob() {
        // Les six métacaractères de globset, tous neutralisés par une classe à
        // un caractère — jamais par un antislash, qui serait détruit par la
        // réécriture `\` -> `/` faite au moment de la compilation du glob.
        let profil_exotique = |name: &str| match name {
            "USERPROFILE" | "TEMP" => Some(r"C:\Users\a?b*c[d]e{f}g".to_string()),
            _ => None,
        };
        let got = expand_env_with(r"%TEMP%\*", &profil_exotique).unwrap();
        assert_eq!(got, r"C:\Users\a[?]b[*]c[[]d[]]e[{]f[}]g\*");
    }

    #[test]
    fn une_regle_dont_le_profil_contient_des_crochets_se_charge() {
        let src = toml_one(
            r#"id = "x.y"
category = "Système"
label = "Test"
paths = ["%TEMP%\\**\\*"]
exclude = []
risk = "low""#,
        );
        let profil_crochets = |name: &str| match name {
            "USERPROFILE" | "TEMP" => Some(r"C:\Users\a[b]c".to_string()),
            _ => None,
        };
        assert!(load_rules_with(&src, &profil_crochets).is_ok());
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
    fn long_path_convertit_un_chemin_court_en_long() {
        use windows::Win32::Storage::FileSystem::GetShortPathNameW;

        let dir = tempfile::tempdir().unwrap();
        let long_dir = dir.path().join("Nom Long Avec Espaces");
        std::fs::create_dir(&long_dir).unwrap();
        // `tempfile` construit son chemin à partir de %TEMP% tel quel, qui peut
        // déjà être une forme courte 8.3 sur cette machine : on passe par
        // `canonicalize` pour obtenir une référence longue indépendante de
        // `long_path`, la fonction sous test.
        let canonical = std::fs::canonicalize(&long_dir).unwrap();
        let long_dir = canonical
            .to_str()
            .unwrap()
            .trim_start_matches(r"\\?\")
            .to_string();

        let mut wide: Vec<u16> = long_dir.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = vec![0u16; 260];
        let len = unsafe {
            GetShortPathNameW(
                windows::core::PCWSTR(wide.as_mut_ptr()),
                Some(&mut buf),
            )
        };
        assert!(len > 0, "GetShortPathNameW a échoué");
        let short: String = String::from_utf16_lossy(&buf[..len as usize]);

        if short.eq_ignore_ascii_case(&long_dir) {
            // Génération de noms 8.3 désactivée sur ce volume : rien à convertir.
            assert_eq!(long_path(&long_dir), long_dir);
            return;
        }

        let got = long_path(&short);
        let got_trim = got.trim_end_matches('\\');
        let want_trim = long_dir.trim_end_matches('\\');
        assert!(
            got_trim.eq_ignore_ascii_case(want_trim),
            "got={got_trim} want={want_trim}"
        );
    }

    #[test]
    fn long_path_from_buffer_refuse_une_longueur_hors_bornes() {
        // `GetLongPathNameW` peut rendre une longueur supérieure au tampon si
        // le chemin s'est allongé entre les deux appels : découper le tampon
        // paniquerait, et `panic = "abort"` tue le processus.
        let buf = [0x41u16, 0x42];
        assert_eq!(long_path_from_buffer(&buf, 5), None);
        assert_eq!(long_path_from_buffer(&buf, 0), None);
        assert_eq!(long_path_from_buffer(&buf, 2), Some("AB".to_string()));
        assert_eq!(long_path_from_buffer(&buf, 1), Some("A".to_string()));
    }

    #[test]
    fn long_path_rend_l_entree_inchangee_si_le_chemin_n_existe_pas() {
        let input = r"C:\chemin\qui\n\existe\pas\ABCDEF~1";
        assert_eq!(long_path(input), input);
    }

    #[test]
    fn les_regles_embarquees_se_chargent_avec_l_environnement_reel() {
        let rules = load_rules_with(RULES_TOML, &system_env).unwrap();
        assert_eq!(rules.len(), 8);
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
