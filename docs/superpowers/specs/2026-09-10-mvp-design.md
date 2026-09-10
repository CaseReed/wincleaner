# WinCleaner MVP — Design

> **Note:** this is the original French design/plan document, kept as a historical record. The project itself is now entirely in English.

Date : 2026-09-10. Nom de travail : `wincleaner` (renommable). Licence : MIT.

## Objectif

Un nettoyeur Windows open source, moderne, sans télémétrie, couvrant l'essentiel
de CCleaner : nettoyage de fichiers inutiles et gestion des programmes au démarrage.
Le MVP tourne sans droits administrateur et n'agit que dans le profil utilisateur.

Hors périmètre MVP : nettoyeur de registre (jamais), désinstallateur, import
winapp2.ini, planification, icône de zone de notification, élévation UAC.

## Stack

- Tauri 2, back-end Rust (édition 2021, stable).
- Front React + TypeScript + Tailwind + shadcn/ui, bundler Vite.
- Crates : `tauri`, `serde`, `toml`, `walkdir`, `globset`, `trash`,
  `windows-registry` (ou `winreg`), `sysinfo` (détection navigateurs ouverts).

## Arborescence

```
wincleaner/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs       enregistre les commandes Tauri
│   │   ├── rules.rs      chargement + validation de rules.toml
│   │   ├── scan.rs       résolution des globs, parcours, tailles
│   │   ├── clean.rs      suppression corbeille ou définitive
│   │   └── startup.rs    gestionnaire de démarrage
│   └── rules.toml        embarqué via include_str!
└── src/                  React
```

## Règles de nettoyage (rules.toml)

```toml
[[rule]]
id = "windows.temp"          # unique, kebab/dot
category = "Système"
label = "Fichiers temporaires"
paths = ["%TEMP%\**\*"]    # globs, variables d'environnement Windows
exclude = []                 # globs exclus
risk = "low"                 # low | medium
```

Validation au chargement (échec = l'appli refuse de démarrer, message clair) :
- chaque chemin commence par une variable parmi `%TEMP%`, `%LOCALAPPDATA%`,
  `%APPDATA%`, `%USERPROFILE%` ;
- après résolution, le chemin est sous `%USERPROFILE%` (canonicalisé, sans `..`) ;
- `id` unique, `risk` dans l'énumération.

Règles initiales (toutes profil utilisateur) :
`windows.temp`, `windows.recycle-bin`, `windows.thumbnails`,
`windows.explorer-recent`, `edge.cache`, `chrome.cache`, `firefox.cache`
(profils multiples via glob `Profiles\*\cache2`), `windows.user-logs`.
La corbeille est un cas spécial : vidée via l'API shell `SHEmptyRecycleBin`,
pas par parcours de fichiers.

## Commandes Tauri

- `scan(rule_ids: Vec<String>) -> Vec<ScanResult>`
  `ScanResult { rule_id, file_count, total_bytes, paths: Vec<String>, skipped: u32 }`
  Fichiers verrouillés ou refusés : ignorés, comptés dans `skipped`.
- `clean(rule_ids, mode: "trash" | "permanent" | "auto") -> CleanReport`
  Re-scan interne juste avant suppression. `auto` = définitif pour `risk = low`,
  corbeille pour `risk = medium`. `CleanReport { freed_bytes, deleted, skipped: Vec<{path, reason}> }`
- `running_browsers() -> Vec<String>` pour l'avertissement.
- `list_startup() -> Vec<StartupEntry>`
  `StartupEntry { id, name, command, source: "run" | "run-once" | "folder", enabled }`
- `set_startup_enabled(id, enabled: bool)`

## Gestionnaire de démarrage

Sources, toutes HKCU ou profil :
- `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` ; `RunOnce` en lecture seule.
- `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\*.lnk`.
- État dans `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`
  et `\StartupFolder` : valeur binaire de 12 octets, octet 0 = 0x02 activé, 0x03
  désactivé, octets 4..12 = FILETIME de la désactivation. Absence de valeur = activé.
Le MVP désactive, il ne supprime jamais une entrée.

## Interface

Fenêtre unique, barre latérale : Nettoyage, Démarrage. Thème clair/sombre.
- Nettoyage : règles groupées par catégorie avec cases, bouton Analyser ; après scan,
  taille et nombre par règle, dépliant listant les chemins, total en tête, bouton
  Nettoyer avec choix du mode (auto par défaut). Rapport final dans un panneau.
  Bandeau d'avertissement si un navigateur ciblé est ouvert.
- Démarrage : tableau nom, commande, source, interrupteur activé/désactivé.

## Sécurité et erreurs

- Aucun accès réseau, aucune télémétrie.
- Toute suppression passe par les règles validées ; pas de commande « supprimer ce chemin ».
- Erreurs par fichier non bloquantes ; erreurs de chargement de règles bloquantes.
- Le front ne reçoit jamais de chemins à supprimer, seulement des `rule_ids`.

## Tests

- Rust : unitaires sur parseur/validation de règles (chemins hors profil refusés),
  intégration scan/clean sur un répertoire temporaire créé par le test, encodage
  et décodage du blob StartupApproved.
- Front : Vitest sur les composants de liste et de rapport.
- Commandes de vérification : `cargo test` dans `src-tauri`, `npm test` à la racine,
  `npm run tauri build` pour le binaire.

## Post-MVP : signature du binaire

Smart App Control (actif par défaut sur les Windows 11 récents) bloque tout exécutable
non signé par une autorité du Trusted Root Program. Avant toute distribution publique,
signer l'exécutable et l'installeur (SignPath, gratuit pour l'open source, ou Azure
Trusted Signing). Hors périmètre MVP ; le développement se fait avec SAC désactivé.
