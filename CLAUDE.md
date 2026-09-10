# WinCleaner — notes pour Claude Code

Nettoyeur Windows open source (MIT). Tauri 2 + Rust (`src-tauri/`), React 19 + TS + Tailwind v4 + shadcn/ui (`src/`).
Spec : `docs/superpowers/specs/2026-09-10-mvp-design.md`. Checklist manuelle : `docs/verification-manuelle.md`.

## Vérifications (à lancer avant de dire « terminé »)
- Rust : `cd src-tauri && cargo test -- --test-threads=1` (single-thread : les tests registre partagent `HKCU\Software\wincleaner-test`). Rust vit dans `%USERPROFILE%\.cargo\bin`.
- Front : `npm test` (Vitest), `npm run build` (tsc + vite).
- Binaire : `npm run tauri build` → `src-tauri/target/release/` (exe, MSI, NSIS).

## Invariants à ne pas casser
- Aucune commande Tauri ne prend un chemin : le front n'envoie que des `rule_ids` et un mode. `clean` re-scanne avant de supprimer.
- Toute règle vit dans `src-tauri/rules.toml` ; variables autorisées : TEMP, LOCALAPPDATA, APPDATA, USERPROFILE ; chemin résolu obligatoirement sous le profil. Les valeurs de variables sont passées par `globset::escape` (un `[` dans un nom de compte faisait sortir la marche du profil).
- Le confinement textuel du chargement ne suffit pas : il est **rejoué sur le disque**. `scan.rs::racine_confinee` refuse toute racine porteuse de `FILE_ATTRIBUTE_REPARSE_POINT` ou dont la forme `canonicalize` sort du profil canonique ; `clean.rs::chemin_supprimable` exige, juste avant chaque suppression, un fichier régulier non point d'analyse résolu sous le profil. Ne jamais marcher ni supprimer sans passer par ces deux gardes (walkdir descend dans sa racine même quand c'est une jonction, et `mklink /J` n'exige aucun privilège).
- Une erreur de chargement propre à la machine (`MissingVar`, `OutsideProfile`) **désactive la règle** (`Rule::unavailable_reason`) sans empêcher le démarrage ; seules les erreurs structurelles de `rules.toml` restent bloquantes.
- `default_checked = false` dans `rules.toml` pour tout ce qui est irréversible ou curé à la main (corbeille, éléments récents, vidages sur incident). Le bouton Nettoyer passe obligatoirement par une confirmation qui énumère l'irréversible.
- La règle `recycle-bin` est toujours nettoyée en premier (`commands.rs::ordre_de_nettoyage`) : sinon elle détruit définitivement ce que les règles précédentes viennent de mettre à la corbeille.
- `%TEMP%` peut être un chemin court 8.3 : `system_env` le résout en long via `GetLongPathNameW`.
- Jamais de nettoyeur de registre. Démarrage : on écrit seulement le blob `StartupApproved` (bit 0 = désactivé), jamais de suppression, RunOnce en lecture seule.
- Tests : jamais sur le vrai profil, la vraie corbeille ni les vraies clés Run. `TempDir` et `HKCU\Software\wincleaner-test` uniquement.
- CSP sans réseau : `default-src 'self'; connect-src 'self' ipc: http://ipc.localhost; style-src 'self'; style-src-attr 'unsafe-inline'; object-src/base-uri/frame-ancestors/form-action 'none'`. `devCsp` distincte pour le HMR de Vite. Capacités limitées à `core:event:default` + `core:window:allow-set-theme`. `src/lib/tauri-config.test.ts` échoue si la politique est relâchée.
- Polices et icônes embarquées (fontsource, lucide).
- Commandes lourdes (`scan`, `clean`, démarrage) en `async` + `spawn_blocking` : une commande sync bloque la fenêtre.

## Machine de dev
- Smart App Control doit être désactivé pour que cargo fonctionne (il bloque tout binaire compilé localement). Vérifier : `Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' | Select VerifiedAndReputablePolicyState` (0 = off).
- Capture de la fenêtre sans permission : PowerShell + `System.Drawing` `CopyFromScreen` (voir `docs/verification-manuelle.md` ou l'historique git).
- Post-MVP : signer l'exe (SignPath / Azure Trusted Signing) avant toute distribution, sinon SAC le bloque chez les utilisateurs.
