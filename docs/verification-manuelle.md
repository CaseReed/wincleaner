# Vérifications manuelles

Ces vérifications ne peuvent pas être automatisées sans détruire des données
de la machine de développement. Elles sont à rejouer avant chaque release.

## VM-1 — Vidage de la corbeille — à exécuter par l'utilisateur

Non exécuté automatiquement : vide réellement la corbeille Windows.

1. Créer un fichier jetable sur le Bureau, puis le supprimer avec la touche
   Suppr (il part dans la corbeille).
2. Ouvrir la corbeille et noter le nombre d'éléments et la taille.
3. Lancer `npm run tauri dev`, écran Nettoyage.
4. Cocher « Corbeille » — elle est **décochée par défaut** — et décocher tout
   le reste, cliquer Analyser.
5. Vérifier que le nombre d'éléments et la taille affichés correspondent à
   ce qui a été noté à l'étape 2.
6. Cliquer Nettoyer en mode « auto », puis « Confirmer le nettoyage » dans la
   barre d'action. La confirmation doit nommer « Corbeille » comme irréversible.
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
4. Mode « auto », cliquer Nettoyer puis « Confirmer le nettoyage ».
5. Vérifier que le fichier a disparu et que le rapport indique des octets libérés.
   Le risque de la règle étant « low », le fichier ne doit PAS se trouver dans
   la corbeille.

## VM-3 — Mode corbeille sur une règle à risque moyen — à exécuter par l'utilisateur

Non exécuté automatiquement : déplace réellement des éléments récents vers
la corbeille.

1. Écran Nettoyage, ne cocher que « Éléments récents » — elle est **décochée
   par défaut** —, Analyser.
2. Mode « auto », Nettoyer puis « Confirmer le nettoyage ». La confirmation ne
   doit PAS nommer « Éléments récents » : en Auto, une règle de risque moyen
   part à la corbeille et reste récupérable.
3. Ouvrir la corbeille : les éléments supprimés doivent s'y trouver
   (règle de risque « medium »).

## VM-4 — Fichier verrouillé par un navigateur — à exécuter par l'utilisateur

Non exécuté automatiquement : supprime réellement le cache Edge.

1. Ouvrir Microsoft Edge et charger quelques pages.
2. Écran Nettoyage : le bandeau « Navigateur ouvert : msedge.exe » doit apparaître.
3. Cocher « Cache Microsoft Edge », Analyser, Nettoyer, Confirmer.
4. Vérifier que l'application ne plante pas, que le rapport liste dans
   « Ignorés » les fichiers verrouillés avec leur message d'erreur, et que
   Edge continue de fonctionner normalement.

## VM-5 — Désactivation d'un programme au démarrage — à exécuter par l'utilisateur

Non exécuté automatiquement : modifie un état réel de démarrage de la machine.

1. Écran Démarrage : la liste doit être un **sous-ensemble exact** de l'onglet
   Démarrage du Gestionnaire des tâches, restreint aux entrées de l'utilisateur
   courant. Les entrées `HKLM` (y compris WOW6432Node), le dossier Démarrage
   commun (`%PROGRAMDATA%\Microsoft\Windows\Start Menu\Programs\StartUp`) et
   les tâches planifiées « au démarrage » sont hors périmètre MVP (elles
   demanderaient une élévation) : leur présence dans le Gestionnaire des tâches
   et leur absence de la liste est le comportement attendu. Formulée comme une
   correspondance stricte, cette étape échouerait toujours sur un poste réel et
   ne prouverait rien.
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

## VM-10 — Confinement au profil face à une jonction — à exécuter par l'utilisateur

Non exécuté automatiquement : crée une jonction sur un vrai `%TEMP%`. La
version automatisée du même invariant vit dans `scan.rs`
(`une_racine_qui_est_une_jonction_hors_profil_nest_pas_parcourue`).

1. Créer `D:\wc-test\precieux.txt` (ou tout autre volume/dossier hors du
   profil) avec quelques octets.
2. Renommer `%LOCALAPPDATA%\Temp` en `Temp.bak`, puis
   `mklink /J "%LOCALAPPDATA%\Temp" "D:\wc-test"` (aucun privilège requis).
3. Lancer l'application, écran Nettoyage, cocher « Fichiers temporaires »,
   Analyser.
4. Vérifier que la règle rend **0 octet** et signale « 1 ignoré », et que
   `precieux.txt` existe toujours après un Nettoyer/Confirmer.
5. Supprimer la jonction (`rmdir "%LOCALAPPDATA%\Temp"`, qui n'efface que le
   lien) et restaurer `Temp.bak`.

## VM-11 — Règle indisponible sur ce poste — à exécuter par l'utilisateur

1. `setx TEMP D:\Temp` (ou tout chemin hors du profil), ouvrir une nouvelle
   session Windows.
2. Lancer le binaire release : la fenêtre doit **s'ouvrir**. Aucune boîte
   d'erreur, aucune sortie en code 1.
3. Écran Nettoyage : « Fichiers temporaires » doit être grisée, décochée, non
   cliquable, avec « Indisponible sur ce poste : … sort du profil utilisateur ».
4. Les autres règles doivent rester utilisables.
5. Rétablir `TEMP`.

## VM-8 — Absence de réseau

1. Lancer l'application, ouvrir le Moniteur de ressources Windows, onglet Réseau.
2. Parcourir les deux écrans, analyser, nettoyer.
3. Vérifier qu'aucune connexion sortante n'est attribuée au processus WinCleaner.

## VM-9 — Thème

1. Basculer « Thème sombre » / « Thème clair » : les deux écrans doivent rester
   lisibles, sans texte sombre sur fond sombre.
