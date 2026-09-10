# WinCleaner

Nettoyeur Windows open source, sans télémétrie et sans accès réseau. Il nettoie
les fichiers inutiles du profil utilisateur et gère les programmes au démarrage
de l'utilisateur courant. Aucun droit administrateur n'est requis : l'application
n'agit que sous `%USERPROFILE%`.

WinCleaner ne contient pas de « nettoyeur de registre » et n'en contiendra jamais.

## Prérequis

- Windows 11, WebView2 installé
- Rust stable MSVC (rustup), Node 24 et npm 11
- Visual Studio 2022 avec les outils C++

## Développement

    npm install
    npm run tauri dev

## Vérification

    npm test
    cd src-tauri
    cargo test

Les vérifications qui ne peuvent pas être automatisées sans détruire des
données (vidage de la corbeille, désactivation réelle d'un programme au
démarrage) sont décrites dans `docs/verification-manuelle.md` et sont à
rejouer avant chaque release.

## Build

    npm run tauri build

## Licence

MIT — voir `LICENSE`.
