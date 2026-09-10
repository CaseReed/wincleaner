# Vérifications manuelles

Ces vérifications ne peuvent pas être automatisées sans détruire des données
de la machine de développement. Elles sont à rejouer avant chaque release.

## VM-1 — Vidage de la corbeille — à exécuter par l'utilisateur

Non exécuté automatiquement : vide réellement la corbeille Windows.

1. Créer un fichier jetable sur le Bureau, puis le supprimer avec la touche
   Suppr (il part dans la corbeille).
2. Ouvrir la corbeille et noter le nombre d'éléments et la taille.
3. Lancer `npm run tauri dev`, écran Nettoyage.
4. Cocher uniquement « Corbeille », cliquer Analyser.
5. Vérifier que le nombre d'éléments et la taille affichés correspondent à
   ce qui a été noté à l'étape 2.
6. Cliquer Nettoyer en mode « auto ».
7. Vérifier que la corbeille Windows est vide et que le rapport indique le
   nombre d'éléments supprimés, sans entrée dans « Ignorés ».

Résultat attendu : aucune boîte de dialogue de confirmation, aucun son,
aucune barre de progression Windows (flags SHERB_NOCONFIRMATION |
SHERB_NOPROGRESSUI | SHERB_NOSOUND).

## VM-2 — Nettoyage des fichiers temporaires — à exécuter par l'utilisateur

Non exécuté automatiquement : supprime réellement un fichier (étape 4-5).
L'étape 1-3 (Analyser) a été vérifiée en lecture seule, voir le rapport de
la Tâche 10.

1. Créer `%TEMP%\wincleaner-probe.txt` avec quelques octets.
2. Écran Nettoyage, ne cocher que « Fichiers temporaires », cliquer Analyser.
3. Déplier les chemins et vérifier que `wincleaner-probe.txt` y figure.
4. Mode « auto », cliquer Nettoyer.
5. Vérifier que le fichier a disparu et que le rapport indique des octets libérés.
   Le risque de la règle étant « low », le fichier ne doit PAS se trouver dans
   la corbeille.

## VM-3 — Mode corbeille sur une règle à risque moyen — à exécuter par l'utilisateur

Non exécuté automatiquement : déplace réellement des éléments récents vers
la corbeille.

1. Écran Nettoyage, ne cocher que « Éléments récents », Analyser.
2. Mode « auto », Nettoyer.
3. Ouvrir la corbeille : les éléments supprimés doivent s'y trouver
   (règle de risque « medium »).

## VM-4 — Fichier verrouillé par un navigateur — à exécuter par l'utilisateur

Non exécuté automatiquement : supprime réellement le cache Edge.

1. Ouvrir Microsoft Edge et charger quelques pages.
2. Écran Nettoyage : le bandeau « Navigateur ouvert : msedge.exe » doit apparaître.
3. Cocher « Cache Microsoft Edge », Analyser, Nettoyer.
4. Vérifier que l'application ne plante pas, que le rapport liste dans
   « Ignorés » les fichiers verrouillés avec leur message d'erreur, et que
   Edge continue de fonctionner normalement.

## VM-5 — Désactivation d'un programme au démarrage — à exécuter par l'utilisateur

Non exécuté automatiquement : modifie un état réel de démarrage de la machine.

1. Écran Démarrage : la liste doit correspondre à l'onglet Démarrage du
   Gestionnaire des tâches (entrées de l'utilisateur courant).
2. Basculer une entrée de Run sur « désactivé ».
3. Ouvrir le Gestionnaire des tâches, onglet Applications de démarrage :
   l'entrée doit y apparaître « Désactivé ».
4. Vérifier dans `regedit` que la valeur existe toujours sous
   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` : elle ne doit PAS
   avoir été supprimée.
5. Réactiver l'entrée et vérifier que le Gestionnaire des tâches repasse à
   « Activé ».

Résultat attendu : l'entrée bascule dans le Gestionnaire des tâches sans
jamais disparaître du registre, dans les deux sens (désactivation puis
réactivation).

## VM-6 — RunOnce en lecture seule

1. Écran Démarrage : toute entrée de source « Registre (RunOnce) » doit avoir
   son interrupteur grisé et non cliquable.

## VM-7 — Règles invalides bloquantes — à exécuter sur le binaire release

Le binaire release est compilé avec `windows_subsystem = "windows"` : il n'a
pas de console, la sortie d'erreur n'est visible nulle part. La porte de
démarrage doit donc passer par une boîte de dialogue. Vérifier sur le binaire
release, pas en `dev`, sinon le test ne prouve rien.

1. Modifier temporairement `src-tauri/rules.toml` : mettre `risk = "high"`
   sur la première règle.
2. `npm run tauri build`.
3. Lancer `src-tauri/target/release/WinCleaner.exe` depuis l'Explorateur
   (double-clic, pas depuis un terminal).
4. Vérifier qu'une boîte de dialogue « WinCleaner » à icône d'erreur apparaît,
   qu'elle nomme `rules.toml` et reprend le message d'erreur, et que la
   fenêtre principale ne s'ouvre pas.
5. Fermer la boîte : le processus doit se terminer (code de sortie 1).
6. Rétablir `rules.toml` (`git checkout -- src-tauri/rules.toml`) et
   reconstruire.

Le texte de la boîte est construit par `wincleaner_lib::startup_error_message`,
couvert par le test `le_message_de_la_porte_de_demarrage_reprend_lerreur`.
L'affichage lui-même (`MessageBoxW`) n'est pas testable automatiquement.

## VM-8 — Absence de réseau

1. Lancer l'application, ouvrir le Moniteur de ressources Windows, onglet Réseau.
2. Parcourir les deux écrans, analyser, nettoyer.
3. Vérifier qu'aucune connexion sortante n'est attribuée au processus WinCleaner.

## VM-9 — Thème

1. Basculer « Thème sombre » / « Thème clair » : les deux écrans doivent rester
   lisibles, sans texte sombre sur fond sombre.
