use crate::rules::{
    profile_canon_with, resolved_excludes_with, resolved_paths_with, system_env, EnvLookup, Rule,
    RuleError, RuleKind,
};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub rule_id: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub paths: Vec<String>,
    pub skipped: u32,
}

/// Interrogation de la corbeille, injectée pour rester testable.
/// Renvoie `(file_count, total_bytes)`.
pub type RecycleQuery<'a> = &'a dyn Fn() -> Result<(u64, u64), String>;

/// `globset` traite `\` comme un caractère d'échappement : on compare donc
/// toujours motifs et chemins en notation `/`.
pub fn to_slash(path: &str) -> String {
    path.replace('\\', "/")
}

/// Métacaractères que `globset::escape` neutralise en les
/// enfermant dans une classe à un caractère.
const ESCAPABLE: [char; 6] = ['?', '*', '[', ']', '{', '}'];

/// Plus long préfixe du motif ne contenant aucun métacaractère de glob.
/// C'est la racine à partir de laquelle `walkdir` descend.
///
/// Les séquences `[c]` produites par `globset::escape` sont des
/// littéraux, pas des jokers : les traiter comme des jokers ferait remonter la
/// racine (un profil nommé `a[b]c` ramènerait la marche à `C:/Users`, donc à
/// tous les profils de la machine).
pub fn glob_root(pattern_slash: &str) -> String {
    let mut literal = String::new();
    let mut chars = pattern_slash.chars().peekable();
    let mut coupe = false;

    while let Some(c) = chars.next() {
        match c {
            '[' => {
                // `[c]` avec c métacaractère : littéral échappé. Tout le reste
                // est une vraie classe de caractères, donc un joker.
                let mut suite = chars.clone();
                match (suite.next(), suite.next()) {
                    (Some(lit), Some(']')) if ESCAPABLE.contains(&lit) => {
                        literal.push(lit);
                        chars.next();
                        chars.next();
                    }
                    _ => {
                        coupe = true;
                        break;
                    }
                }
            }
            '*' | '?' | '{' => {
                coupe = true;
                break;
            }
            c => literal.push(c),
        }
    }

    if coupe {
        match literal.rfind('/') {
            Some(i) => literal.truncate(i),
            None => literal.clear(),
        }
    }
    literal
}

pub(crate) fn build_set(patterns: &[String]) -> Result<GlobSet, RuleError> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        // `literal_separator` : un `*` isolé ne doit pas franchir de `/`
        // (sinon `%TEMP%\*.txt`, non récursif, descendrait quand même dans
        // les sous-répertoires). `**` reste traité spécialement par globset
        // et continue de traverser plusieurs niveaux.
        let glob = GlobBuilder::new(&to_slash(p))
            .literal_separator(true)
            .build()
            .map_err(|e| RuleError::Glob {
                pattern: p.clone(),
                cause: e.to_string(),
            })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| RuleError::Glob {
        pattern: patterns.join(", "),
        cause: e.to_string(),
    })
}

/// `FILE_ATTRIBUTE_REPARSE_POINT` : jonction, lien symbolique, point de
/// montage de volume, espace réservé de synchronisation cloud. Tous font
/// qu'un chemin ne désigne pas le répertoire qu'il a l'air de désigner.
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

pub(crate) fn est_point_danalyse(md: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        md.file_type().is_symlink()
    }
}

/// Verdict du confinement d'une racine de marche.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Racine {
    /// Répertoire réel, résolu sous le profil : on peut marcher.
    Marchable,
    /// Absente du disque : la règle ne s'applique pas sur cette machine.
    /// Ce n'est pas une anomalie, rien n'est compté.
    Absente,
    /// Point d'analyse, ou chemin résolu hors du profil : on ne marche pas.
    Refusee,
}

/// Confronte une racine de marche au disque avant d'y descendre.
///
/// C'est le cœur du confinement : la vérification faite au chargement des
/// règles est textuelle, et `walkdir` descend dans sa racine de marche même
/// quand celle-ci est un point d'analyse. Une jonction posée sur `%TEMP%` —
/// que `mklink /J` crée sans aucun privilège — suffirait sinon à faire
/// supprimer un arbre entier hors du profil, voire hors du volume système.
pub(crate) fn racine_confinee(win_root: &str, profile_canon: &Path) -> Racine {
    let md = match std::fs::symlink_metadata(win_root) {
        Ok(md) => md,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Racine::Absente,
        Err(_) => return Racine::Refusee,
    };
    // Refusé même quand la cible reste sous le profil : on ne marche que sur
    // des répertoires réels, jamais sur une indirection.
    if est_point_danalyse(&md) {
        return Racine::Refusee;
    }
    match std::fs::canonicalize(win_root) {
        Ok(reel) if reel.starts_with(profile_canon) => Racine::Marchable,
        Ok(_) => Racine::Refusee,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Racine::Absente,
        Err(_) => Racine::Refusee,
    }
}

/// Racines de marche minimales d'un jeu de motifs : une racine incluse dans
/// une autre serait parcourue deux fois.
pub(crate) fn walk_roots(patterns: &[String]) -> Vec<String> {
    let mut roots: Vec<String> = patterns.iter().map(|p| glob_root(&to_slash(p))).collect();
    roots.sort();
    roots.dedup();
    roots
        .iter()
        .filter(|r| {
            !roots
                .iter()
                .any(|other| other != *r && r.starts_with(&format!("{other}/")))
        })
        .cloned()
        .collect()
}

/// Parcourt les motifs de la règle et renvoie, pour chaque fichier retenu,
/// son chemin Windows et sa taille. Les entrées illisibles, les racines
/// refusées et les points d'analyse rencontrés sont comptés dans `skipped`
/// et jamais propagés.
fn collect(
    patterns: &[String],
    excludes: &[String],
    profile_canon: &Path,
) -> Result<(BTreeMap<String, u64>, u32), RuleError> {
    let include = build_set(patterns)?;
    let exclude = build_set(excludes)?;
    let mut found: BTreeMap<String, u64> = BTreeMap::new();
    let mut skipped: u32 = 0;

    for root in walk_roots(patterns) {
        let win_root = root.replace('/', "\\");
        match racine_confinee(&win_root, profile_canon) {
            Racine::Marchable => {}
            Racine::Absente => continue,
            Racine::Refusee => {
                skipped += 1;
                continue;
            }
        }

        // `follow_links(false)` arrête déjà jonctions et liens symboliques
        // sous la racine ; `filter_entry` ferme les autres points d'analyse
        // (espaces réservés cloud, conteneurs), que Rust ne classe pas comme
        // des liens et dans lesquels `walkdir` descendrait.
        let sautees = std::cell::Cell::new(0u32);
        let marche = WalkDir::new(&win_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() > 0 && e.file_type().is_dir() {
                    if let Ok(md) = e.metadata() {
                        if est_point_danalyse(&md) {
                            sautees.set(sautees.get() + 1);
                            return false;
                        }
                    }
                }
                true
            });

        for entry in marche {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    // Un répertoire racine absent n'est pas une anomalie :
                    // la règle ne s'applique simplement pas sur cette machine.
                    if err.io_error().map(|e| e.kind()) == Some(std::io::ErrorKind::NotFound) {
                        continue;
                    }
                    skipped += 1;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let win_path = entry.path().to_string_lossy().to_string();
            let slash = to_slash(&win_path);
            if !include.is_match(&slash) || exclude.is_match(&slash) {
                continue;
            }
            match entry.metadata() {
                Ok(md) => {
                    found.insert(win_path, md.len());
                }
                Err(_) => skipped += 1,
            }
        }
        skipped += sautees.get();
    }
    Ok((found, skipped))
}

pub fn scan_rule_with_api(
    rule: &Rule,
    lookup: EnvLookup,
    recycle: RecycleQuery,
) -> Result<ScanResult, RuleError> {
    if rule.kind == RuleKind::RecycleBin {
        return Ok(match recycle() {
            Ok((count, bytes)) => ScanResult {
                rule_id: rule.id.clone(),
                file_count: count,
                total_bytes: bytes,
                paths: Vec::new(),
                skipped: 0,
            },
            Err(_) => ScanResult {
                rule_id: rule.id.clone(),
                file_count: 0,
                total_bytes: 0,
                paths: Vec::new(),
                skipped: 1,
            },
        });
    }

    let patterns = resolved_paths_with(rule, lookup)?;
    let excludes = resolved_excludes_with(rule, lookup)?;
    let profile_canon = profile_canon_with(lookup)?;
    let (found, skipped) = collect(&patterns, &excludes, &profile_canon)?;
    Ok(ScanResult {
        rule_id: rule.id.clone(),
        file_count: found.len() as u64,
        total_bytes: found.values().sum(),
        paths: found.keys().cloned().collect(),
        skipped,
    })
}

pub fn scan_rule(rule: &Rule) -> Result<ScanResult, RuleError> {
    scan_rule_with_api(rule, &system_env, &query_recycle_bin)
}

/// Interroge la corbeille de tous les volumes. Lecture seule.
/// Renvoie `(nombre d'éléments, octets occupés)`.
pub fn query_recycle_bin() -> Result<(u64, u64), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };
    // PCWSTR::null() => tous les volumes du poste.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }
        .map_err(|e| format!("SHQueryRecycleBinW a échoué : {e}"))?;
    Ok((info.i64NumItems.max(0) as u64, info.i64Size.max(0) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Risk, Rule, RuleKind};
    use std::fs;
    use tempfile::TempDir;

    /// Crée un faux profil :
    ///   <tmp>\AppData\Local\Temp\a.txt      (3 octets)
    ///   <tmp>\AppData\Local\Temp\sub\b.txt  (5 octets)
    ///   <tmp>\AppData\Local\Temp\keep.log   (7 octets)
    fn faux_profil() -> TempDir {
        let dir = TempDir::new().unwrap();
        let temp = dir.path().join("AppData").join("Local").join("Temp");
        fs::create_dir_all(temp.join("sub")).unwrap();
        fs::write(temp.join("a.txt"), b"aaa").unwrap();
        fs::write(temp.join("sub").join("b.txt"), b"bbbbb").unwrap();
        fs::write(temp.join("keep.log"), b"ccccccc").unwrap();
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

    fn regle_temp(paths: Vec<&str>, exclude: Vec<&str>) -> Rule {
        Rule {
            id: "windows.temp".into(),
            category: "Système".into(),
            label: "Fichiers temporaires".into(),
            paths: paths.into_iter().map(String::from).collect(),
            exclude: exclude.into_iter().map(String::from).collect(),
            risk: Risk::Low,
            kind: RuleKind::Files,
            default_checked: true,
        }
    }

    fn recycle_absent() -> Result<(u64, u64), String> {
        panic!("l'API corbeille ne doit pas être appelée pour une règle « files »");
    }

    /// Crée une jonction de répertoire. `mklink /J` n'exige aucun privilège,
    /// contrairement à `mklink /D` : le test tourne sans élévation. Le lien et
    /// sa cible vivent tous les deux dans le `TempDir` du test.
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

    /// `<base>/profil` (faux profil), `<base>/dehors/precieux.txt`.
    fn profil_et_dehors(base: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let profil = base.join("profil");
        let dehors = base.join("dehors");
        fs::create_dir_all(profil.join("AppData").join("Local")).unwrap();
        fs::create_dir_all(&dehors).unwrap();
        fs::write(dehors.join("precieux.txt"), b"precieux").unwrap();
        (profil, dehors)
    }

    #[test]
    fn une_racine_qui_est_une_jonction_hors_profil_nest_pas_parcourue() {
        let base = TempDir::new().unwrap();
        let (profil, dehors) = profil_et_dehors(base.path());
        // %TEMP% est une jonction vers un répertoire hors du faux profil :
        // exactement la configuration d'un poste où Temp a été déplacé.
        jonction(&profil.join("AppData").join("Local").join("Temp"), &dehors);

        let lookup = lookup_for(&profil);
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();

        assert_eq!(res.file_count, 0, "paths = {:?}", res.paths);
        assert_eq!(res.total_bytes, 0);
        assert_eq!(res.skipped, 1, "la racine refusée doit être signalée");
        assert!(dehors.join("precieux.txt").exists());
    }

    #[test]
    fn une_jonction_sous_la_racine_nest_pas_suivie() {
        let base = TempDir::new().unwrap();
        let (profil, dehors) = profil_et_dehors(base.path());
        let temp = profil.join("AppData").join("Local").join("Temp");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("a.txt"), b"aaa").unwrap();
        jonction(&temp.join("lien"), &dehors);

        let lookup = lookup_for(&profil);
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();

        assert_eq!(res.file_count, 1, "paths = {:?}", res.paths);
        assert!(res.paths[0].ends_with("a.txt"));
        assert!(res.paths.iter().all(|p| !p.contains("precieux")));
    }

    #[test]
    fn une_racine_qui_est_une_jonction_vers_le_profil_est_refusee_aussi() {
        // Même pointant à l'intérieur du profil, un point d'analyse à la racine
        // de marche est refusé : on ne marche que sur des répertoires réels.
        let base = TempDir::new().unwrap();
        let profil = base.path().join("profil");
        let ailleurs = profil.join("Ailleurs");
        fs::create_dir_all(profil.join("AppData").join("Local")).unwrap();
        fs::create_dir_all(&ailleurs).unwrap();
        fs::write(ailleurs.join("a.txt"), b"aaa").unwrap();
        jonction(&profil.join("AppData").join("Local").join("Temp"), &ailleurs);

        let lookup = lookup_for(&profil);
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();

        assert_eq!(res.file_count, 0, "paths = {:?}", res.paths);
        assert_eq!(res.skipped, 1);
        assert!(ailleurs.join("a.txt").exists());
    }

    #[test]
    fn la_regle_elements_recents_epargne_les_destinations_personnalisees() {
        // CustomDestinations porte les éléments que l'utilisateur a épinglés
        // au clic droit sur une icône de la barre des tâches : des données
        // curées à la main, pas des déchets.
        let dir = TempDir::new().unwrap();
        let recent = dir
            .path()
            .join("AppData")
            .join("Roaming")
            .join("Microsoft")
            .join("Windows")
            .join("Recent");
        fs::create_dir_all(recent.join("AutomaticDestinations")).unwrap();
        fs::create_dir_all(recent.join("CustomDestinations")).unwrap();
        fs::write(recent.join("doc.lnk"), b"aaa").unwrap();
        fs::write(
            recent
                .join("AutomaticDestinations")
                .join("a.automaticDestinations-ms"),
            b"bbbbb",
        )
        .unwrap();
        fs::write(
            recent
                .join("CustomDestinations")
                .join("epingle.customDestinations-ms"),
            b"ccccccc",
        )
        .unwrap();

        // La vraie règle embarquée, pas une copie : c'est elle qu'il s'agit
        // d'empêcher de redescendre dans CustomDestinations.
        let lookup = lookup_for(dir.path());
        let regles = crate::rules::load_rules_with(crate::rules::RULES_TOML, &lookup).unwrap();
        let regle = regles
            .iter()
            .find(|r| r.id == "windows.explorer-recent")
            .unwrap();
        let res = scan_rule_with_api(regle, &lookup, &recycle_absent).unwrap();

        assert_eq!(res.file_count, 2, "paths = {:?}", res.paths);
        assert!(
            res.paths.iter().all(|p| !p.contains("CustomDestinations")),
            "les éléments épinglés doivent rester hors de portée : {:?}",
            res.paths
        );
    }

    #[test]
    fn glob_root_coupe_au_premier_joker() {
        assert_eq!(glob_root("C:/Users/T/Temp/**/*"), "C:/Users/T/Temp");
        assert_eq!(glob_root("C:/Users/T/E/thumb_*.db"), "C:/Users/T/E");
        assert_eq!(glob_root("C:/Users/T/Temp"), "C:/Users/T/Temp");
    }

    #[test]
    fn glob_root_traite_les_metacaracteres_echappes_comme_des_litteraux() {
        // `a[[]b[]]c` est la forme échappée de `a[b]c` : la racine de marche
        // doit être le répertoire réel, pas `C:/Users`.
        assert_eq!(
            glob_root("C:/Users/a[[]b[]]c/Temp/**/*"),
            "C:/Users/a[b]c/Temp"
        );
        // Une vraie classe de caractères écrite par la règle reste un joker.
        assert_eq!(glob_root("C:/Users/T/Temp/[ab]*.txt"), "C:/Users/T/Temp");
    }

    #[test]
    fn un_profil_avec_des_crochets_ne_deborde_pas_sur_le_profil_voisin() {
        // Deux profils frères : le nom du profil visé contient `[` et `]`,
        // qui formeraient une classe de caractères matchant le voisin.
        let base = TempDir::new().unwrap();
        let vise = base.path().join("a[b]c");
        let voisin = base.path().join("abc");
        for profil in [&vise, &voisin] {
            let temp = profil.join("AppData").join("Local").join("Temp");
            fs::create_dir_all(&temp).unwrap();
        }
        fs::write(
            vise.join("AppData").join("Local").join("Temp").join("a.txt"),
            b"aaa",
        )
        .unwrap();
        fs::write(
            voisin
                .join("AppData")
                .join("Local")
                .join("Temp")
                .join("intrus.txt"),
            b"bbbbb",
        )
        .unwrap();

        let lookup = lookup_for(&vise);
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();

        assert_eq!(res.file_count, 1, "paths = {:?}", res.paths);
        assert_eq!(res.total_bytes, 3);
        assert!(res.paths[0].ends_with("a.txt"));
        assert!(
            res.paths.iter().all(|p| !p.contains("intrus.txt")),
            "la marche a débordé sur le profil voisin : {:?}",
            res.paths
        );
    }

    #[test]
    fn scan_compte_les_fichiers_et_les_octets() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();
        assert_eq!(res.rule_id, "windows.temp");
        assert_eq!(res.file_count, 3);
        assert_eq!(res.total_bytes, 3 + 5 + 7);
        assert_eq!(res.skipped, 0);
        assert_eq!(res.paths.len(), 3);
    }

    #[test]
    fn scan_applique_les_exclusions() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![r"%TEMP%\*.log"]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();
        assert_eq!(res.file_count, 2);
        assert_eq!(res.total_bytes, 3 + 5);
    }

    #[test]
    fn scan_ne_retient_pas_les_repertoires() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();
        assert!(res.paths.iter().all(|p| !p.ends_with("sub")));
    }

    #[test]
    fn scan_dun_glob_non_recursif_ne_descend_pas() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(vec![r"%TEMP%\*.txt"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();
        assert_eq!(res.file_count, 1);
        assert_eq!(res.total_bytes, 3);
    }

    #[test]
    fn scan_dun_repertoire_absent_rend_un_resultat_vide() {
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(vec![r"%TEMP%\**\*"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();
        assert_eq!(res.file_count, 0);
        assert_eq!(res.total_bytes, 0);
        assert_eq!(res.skipped, 0);
    }

    #[test]
    fn scan_ne_compte_pas_deux_fois_un_fichier_couvert_par_deux_globs() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = regle_temp(vec![r"%TEMP%\**\*", r"%TEMP%\*.txt"], vec![]);
        let res = scan_rule_with_api(&rule, &lookup, &recycle_absent).unwrap();
        assert_eq!(res.file_count, 3);
        assert_eq!(res.total_bytes, 15);
    }

    #[test]
    fn scan_remonte_une_erreur_de_regle() {
        let rule = regle_temp(vec![r"%WINDIR%\*"], vec![]);
        let dir = TempDir::new().unwrap();
        let lookup = lookup_for(dir.path());
        assert!(scan_rule_with_api(&rule, &lookup, &recycle_absent).is_err());
    }

    #[test]
    fn le_scan_dune_regle_corbeille_utilise_lapi_et_ignore_les_globs() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = Rule {
            id: "windows.recycle-bin".into(),
            category: "Système".into(),
            label: "Corbeille".into(),
            paths: vec![],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::RecycleBin,
            default_checked: false,
        };
        let query = || Ok((7u64, 700u64));
        let res = scan_rule_with_api(&rule, &lookup, &query).unwrap();
        assert_eq!(res.file_count, 7);
        assert_eq!(res.total_bytes, 700);
        assert!(res.paths.is_empty());
        assert_eq!(res.skipped, 0);
    }

    #[test]
    fn un_echec_de_linterrogation_corbeille_compte_un_skipped() {
        let dir = faux_profil();
        let lookup = lookup_for(dir.path());
        let rule = Rule {
            id: "windows.recycle-bin".into(),
            category: "Système".into(),
            label: "Corbeille".into(),
            paths: vec![],
            exclude: vec![],
            risk: Risk::Low,
            kind: RuleKind::RecycleBin,
            default_checked: false,
        };
        let query = || Err("échec".to_string());
        let res = scan_rule_with_api(&rule, &lookup, &query).unwrap();
        assert_eq!(res.file_count, 0);
        assert_eq!(res.total_bytes, 0);
        assert_eq!(res.skipped, 1);
    }

    /// Test couplé à l'environnement : il interroge la corbeille réelle de la
    /// machine, donc son résultat dépend du poste et pas seulement du code.
    /// Il ne vérifie qu'une chose : l'appel FFI ne panique pas et ne renvoie
    /// pas d'erreur. Aucune assertion sur les valeurs, qui varient.
    #[test]
    fn query_recycle_bin_ne_panique_pas() {
        // Appel en lecture seule sur la vraie corbeille : n'efface rien.
        let res = query_recycle_bin();
        assert!(res.is_ok(), "SHQueryRecycleBinW a échoué : {res:?}");
    }
}
