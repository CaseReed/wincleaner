# Vérifications manuelles

Ces vérifications ne peuvent pas être automatisées sans détruire des données
de la machine de développement. Elles sont à rejouer avant chaque release.

## VM-1 — Vidage de la corbeille

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
