import type { Dictionary } from "./en";

/// The French dictionary. Typed as `Dictionary`, so `tsc` refuses a missing or
/// stray key; `i18n.test.ts` checks the same thing at runtime.
export const fr: Dictionary = {
  // Coquille et navigation
  "nav.label": "Navigation principale",
  "nav.clean": "Nettoyage",
  "nav.startup": "Démarrage",
  "nav.settings": "Paramètres",
  "theme.toLight": "Passer au thème clair",
  "theme.toDark": "Passer au thème sombre",
  "theme.light": "Thème clair",
  "theme.dark": "Thème sombre",

  // Communs
  "common.retry": "Réessayer",
  "common.cancel": "Annuler",
  "common.remove": "Supprimer",
  "common.view": "Voir",

  // Bandeau et messages du bac à sable
  "sandbox.banner":
    "Mode bac à sable — le nettoyage ne touche que le profil de test situé dans {root}",
  "sandbox.leave": "Quitter",
  "sandbox.created": "Profil bac à sable créé",
  "sandbox.removed": "Bac à sable supprimé",

  // Écran Nettoyage
  "clean.title": "Nettoyage",
  "clean.rulesError": "Impossible de charger les règles.",
  "clean.hint":
    "Le mode Auto supprime définitivement les éléments à faible risque et envoie le reste à la Corbeille. Changez de mode dans la barre du bas avant de nettoyer.",
  "clean.hintDismiss": "J’ai compris",
  "clean.freed": "Libéré",
  "clean.reclaimable": "Récupérable",
  "clean.filesDeleted": "{count} fichiers supprimés",
  "clean.moreUnchecked": "{bytes} de plus dans les règles non cochées",
  "clean.sortBySize": "Trier par taille",
  "clean.analyze": "Analyser",
  "clean.analyzing": "Analyse…",
  "clean.analyzingRules.one": "Analyse de {count} règle…",
  "clean.analyzingRules.other": "Analyse de {count} règles…",
  "clean.cleaning": "Nettoyage…",
  "clean.cleaningRules.one": "Nettoyage de {count} règle…",
  "clean.cleaningRules.other": "Nettoyage de {count} règles…",
  "clean.progressAnalyzing": "Analyse {counter} · {label}",
  "clean.progressCleaning": "Nettoyage {counter} · {label}",
  "clean.empty": "Lancez une analyse pour mesurer ce qui peut être libéré.",
  "clean.search": "Rechercher une règle",
  "clean.searchEmpty": "Aucune règle ne correspond à votre recherche.",
  "clean.summary":
    "{native} règles intégrées · {detected} règles Winapp2 détectées sur {retained} converties ({dropped} entrées non prises en charge)",
  "clean.clean": "Nettoyer",

  // Avertissement navigateur
  "browser.open":
    "{name} est ouvert : ses fichiers de cache en cours d’utilisation seront ignorés. Fermez-le pour un nettoyage complet.",
  "browser.background":
    "{name} tourne encore en arrière-plan ({processes}) : quittez-le depuis la zone de notification, sans quoi ses fichiers de cache en cours d’utilisation seront ignorés.",
  "browser.processes.one": "{count} processus",
  "browser.processes.other": "{count} processus",

  // Annonces de la zone active
  "announce.analyzing": "Analyse…",
  "announce.cleaning": "Nettoyage…",
  "announce.scanProgress": "Analyse : {done} règles sur {total}, {bytes} jusqu’ici",
  "announce.cleanProgress": "Nettoyage : {done} règles sur {total}, {bytes} libérés jusqu’ici",
  "announce.scanDone.one":
    "Analyse terminée : {bytes} récupérables dans {count} règle sélectionnée",
  "announce.scanDone.other":
    "Analyse terminée : {bytes} récupérables dans {count} règles sélectionnées",
  "announce.cleanDone": "Nettoyage terminé : {bytes} libérés, {count} fichiers supprimés",
  "toast.cleaned": "Nettoyage effectué : {bytes} libérés",

  // Modes de suppression
  "mode.label": "Mode de suppression",
  "mode.auto": "Auto",
  "mode.trash": "Corbeille",
  "mode.permanent": "Définitif",
  "mode.autoHelp":
    "Auto : suppression définitive pour les éléments à faible risque, Corbeille pour le reste.",
  "mode.recycleFirst":
    "La Corbeille est vidée en premier : ce que les autres règles y déposent pendant la même passe n’est pas emporté.",
  "mode.emptyDirs":
    "Les dossiers qu’une règle vide sont supprimés quel que soit le mode : un dossier vide ne contient aucune donnée.",

  // Confirmation
  "confirm.title": "Nettoyer {bytes} en mode {mode} ?",
  "confirm.irreversible": "Sans retour possible : {rules}.",
  "confirm.reversible": "Tout part à la Corbeille et reste récupérable.",
  "confirm.confirm": "Confirmer le nettoyage",

  // Rapport de nettoyage
  "report.title": "Dernier nettoyage",
  "report.summary": "{bytes} libérés · {count} fichiers supprimés",
  "report.skipped": "Ignorés ({count})",

  // Lignes de règles
  "rules.count.one": "{count} règle",
  "rules.count.other": "{count} règles",
  "rules.mediumRisk": "risque moyen",
  "rules.allVolumes": "tous les volumes",
  "rules.skipped": "{count} ignorés",
  "rules.filesSr": " fichiers",
  "rules.unavailable": "Indisponible sur cette machine : {reason}",
  "rules.recycleBinNote":
    "Vide la corbeille de chaque volume de cette machine, y compris hors du profil utilisateur. Définitif et irréversible : le mode de suppression ne s’y applique pas.",
  "rules.tempNote": "Fermez les installateurs en cours avant de nettoyer.",
  "rules.showPaths": "Afficher les chemins",
  "rules.hidePaths": "Masquer les chemins",
  "rules.showPathsOf": "Afficher les chemins de {label}",
  "rules.hidePathsOf": "Masquer les chemins de {label}",
  "rules.pathsOf": "Chemins de {label}",
  "rules.excludeFile": "Exclure ce fichier",
  "rules.excludeFolder": "Exclure son dossier",
  "rules.excludeFileOf": "Exclure {path} de {label}",
  "rules.excludeFolderOf": "Exclure le dossier contenant {path} de {label}",
  "rules.excluded": "Exclu — il ne sera plus nettoyé",
  "rules.staleCounts": "Relancez l’analyse pour actualiser les chiffres",
  "rules.winapp2Attribution":
    "Certaines règles de cette catégorie sont des règles communautaires issues de Winapp2 (CC-BY-SA 4.0) — {url}",

  // Exclusions (Réglages)
  "exclusions.empty":
    "Aucune exclusion. Dans la liste de nettoyage, ouvrez « Afficher les chemins » sur une règle pour en écarter définitivement un fichier ou un dossier.",
  "exclusions.loadFailed": "Les exclusions n’ont pas pu être lues.",
  "exclusions.remove": "Ne plus exclure {pattern}",
  "exclusions.removed": "Exclusion supprimée",
  "exclusions.added": "{pattern} ne sera plus nettoyé",
  "exclusions.addFailed": "Ce chemin n’a pas pu être exclu.",
  "exclusions.addedOn": "Ajoutée le {date}",

  // Jauge de récupération
  "gauge.description": "Récupérable {bytes} : {named}",
  "gauge.more.one": ", et {count} règle plus petite",
  "gauge.more.other": ", et {count} règles plus petites",
  "gauge.segment": "{label} — {bytes}",

  // Verdict du bac à sable
  "verdict.title": "Verdict du bac à sable",
  "verdict.passed": "réussi",
  "verdict.failed": "échoué",
  "verdict.sentinels": "Sentinelles intactes",
  "verdict.junk": "Déchets supprimés",
  "verdict.junkScope.one": "pour {count} règle nettoyée",
  "verdict.junkScope.other": "pour les {count} règles nettoyées",
  "verdict.junctions": "Leurres de jonction intacts",
  "verdict.junctionBroken": "Une jonction posée par le bac à sable n’est plus en place.",
  "verdict.damaged": "Supprimés ou réécrits, alors qu’ils auraient dû survivre",
  "verdict.remaining": "Déchets encore sur le disque",
  "verdict.unreadable":
    "Le nettoyage a bien eu lieu ; la relecture du bac à sable a échoué, il n’y a donc pas de verdict cette fois.",

  // Écran Démarrage
  "startup.title": "Démarrage",
  "startup.subtitle": "Décidez de ce qui démarre avec votre session.",
  "startup.scope":
    "Seules les entrées de votre propre session sont listées : le registre HKCU (Run, RunOnce) et votre dossier Démarrage. Les entrées partagées par tous les utilisateurs et les tâches planifiées demandent une élévation et restent hors de portée.",
  "startup.sandboxTitle": "Indisponible tant que le bac à sable est actif.",
  "startup.sandboxBody":
    "Les programmes de démarrage vivent dans le vrai registre Windows et dans votre vrai dossier Démarrage. Le bac à sable ne touche ni l’un ni l’autre, il n’y a donc rien à montrer ici. Quittez le bac à sable pour les gérer.",
  "startup.error": "Impossible de lire les programmes de démarrage.",
  "startup.empty": "Aucun programme ne démarre avec votre session.",
  "startup.summary.one": "{count} programme, {enabled} activé",
  "startup.summary.other": "{count} programmes, {enabled} activés",
  "startup.caption": "Programmes qui démarrent avec votre session",
  "startup.colName": "Nom",
  "startup.colCommand": "Commande",
  "startup.colSource": "Source",
  "startup.colEnabled": "Activé",
  "startup.readOnly": "Lecture seule",
  "startup.enable": "Activer {name}",
  "startup.enabled": "{name} activé au démarrage",
  "startup.disabled": "{name} désactivé au démarrage",
  "startup.changeFailed": "{name} n’a pas pu être modifié",
  "startup.source.run": "Registre (Run)",
  "startup.source.run-once": "Registre (RunOnce)",
  "startup.source.folder": "Dossier Démarrage",

  // Écran Paramètres
  "settings.title": "Paramètres",
  "settings.subtitle": "Ce qu’est cette version, ce qui a changé et ce dont elle est faite.",
  "settings.about": "À propos",
  "settings.aboutBody":
    "Open source, MIT. Aucune télémétrie et aucun accès réseau, hormis une requête vers GitHub lorsque vous cliquez sur Rechercher des mises à jour ou que vous activez la recherche automatique (désactivée par défaut).",
  "settings.whatsNew": "Nouveautés de la version {version}",
  "settings.updates": "Mises à jour",
  "settings.sandbox": "Bac à sable",
  "settings.exclusions": "Exclusions",
  "settings.notices": "Mentions",
  "settings.language": "Langue",
  "settings.languageSystem": "Système",
  "settings.languageEn": "English",
  "settings.languageFr": "Français",
  "settings.languageNote":
    "Les noms de règles, les notes de version et le journal des modifications viennent de leurs propres sources et restent en anglais.",

  // Section Mises à jour
  "updates.check": "Rechercher des mises à jour",
  "updates.checking": "Recherche…",
  "updates.upToDate": "Vous êtes à jour ({version})",
  "updates.available": "WinCleaner {version} est disponible",
  "updates.published": "Publiée le {date}",
  "updates.copyLink": "Copier le lien",
  "updates.copied": "Lien de la version copié",
  "updates.copyFailed": "Impossible de copier le lien",
  "updates.autoCheck": "Rechercher automatiquement au démarrage",
  "updates.autoCheckPrivacy":
    "Lorsque c’est activé, WinCleaner envoie une requête à api.github.com au démarrage, sans autre identifiant que la version de l’application dans le User-Agent.",
  "updates.error.not-available": "Aucune version publique n’est encore disponible",
  "updates.error.rate-limited": "Limite de requêtes GitHub atteinte, réessayez plus tard",
  "updates.error.malformed": "Impossible de lire la réponse de GitHub",
  "updates.error.offline": "Impossible de joindre GitHub — vérifiez votre connexion",

  // Section Bac à sable
  "sandboxSection.what":
    "Un bac à sable est un profil Windows synthétique que WinCleaner construit dans votre dossier temporaire : les fichiers indésirables que chaque règle doit retirer, plus des documents, des clés et des caches leurres qui doivent survivre.",
  "sandboxSection.active":
    "Tant qu’il est actif, Analyser et Nettoyer s’exécutent pour de vrai sur ce profil et sur rien d’autre — votre vrai profil n’est pas touché, et la Corbeille comme les clés de registre de démarrage restent hors de portée.",
  "sandboxSection.counts":
    "{junk} fichiers indésirables · {sentinels} fichiers qui doivent survivre · {rules} règles Winapp2 détectées",
  "sandboxSection.leave": "Quitter le bac à sable",
  "sandboxSection.removing": "Suppression…",
  "sandboxSection.create": "Créer un profil bac à sable",
  "sandboxSection.creating": "Création…",
  "sandboxSection.orphans.one": "{count} ancien dossier de bac à sable ({bytes})",
  "sandboxSection.orphans.other": "{count} anciens dossiers de bac à sable ({bytes})",
  "sandboxSection.orphansRemoved.one": "{count} ancien dossier de bac à sable supprimé",
  "sandboxSection.orphansRemoved.other": "{count} anciens dossiers de bac à sable supprimés",

  // Section Mentions
  "notices.mit": "WinCleaner est distribué sous licence MIT.",
  "notices.winapp2": "Règles communautaires issues de Winapp2 (CC-BY-SA 4.0) — {url}",
  "notices.bundled":
    "Les polices et les icônes sont fournies avec l’application ; rien n’est téléchargé à l’exécution.",
};
