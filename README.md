# WinCleaner

Nettoyeur Windows open source, sans télémétrie et sans accès réseau. Il nettoie
les fichiers inutiles du profil utilisateur et gère les programmes au démarrage
de l'utilisateur courant. Aucun droit administrateur n'est requis : l'application
n'agit que sous `%USERPROFILE%`, à la seule exception de la règle « Corbeille »
décrite ci-dessous.

Le confinement au profil n'est pas qu'une promesse d'écriture des règles : il
est confronté au disque. Avant de parcourir une racine, et de nouveau avant
chaque suppression, le chemin est résolu par `canonicalize` et doit rester sous
le `%USERPROFILE%` résolu ; tout point d'analyse (jonction, lien symbolique,
point de montage, espace réservé de synchronisation cloud) est refusé plutôt
que suivi, y compris à la racine. Une jonction posée sur `%TEMP%` ne fait donc
rien sortir du profil : la règle rend zéro octet et signale un élément ignoré.

Cette exception : la règle « Corbeille » passe par l'API Windows
`SHEmptyRecycleBinW`, qui vide la corbeille de **tous les volumes** du poste, y
compris hors du profil utilisateur. Cette suppression est définitive, quel que
soit le mode de suppression choisi. L'écran Nettoyage le signale sur la ligne
de la règle, elle est **décochée par défaut**, et elle est toujours traitée en
première position d'une passe — sans quoi elle emporterait définitivement ce
que les autres règles viennent d'y déposer en mode « Corbeille ».

« Nettoyer » ouvre une confirmation qui annonce le volume, le mode, et énumère
nommément ce que ce mode détruira sans retour possible. « Corbeille »,
« Éléments récents » et « Vidages sur incident » sont décochées par défaut.

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
