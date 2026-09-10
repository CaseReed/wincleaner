# Signature du binaire

## Pourquoi signer

Windows 11 avec Smart App Control (SAC) actif bloque l'exécution de tout
binaire compilé localement ou téléchargé qui n'est pas signé par un éditeur
de confiance. Sans signature, WinCleaner ne se lance tout simplement pas chez
un utilisateur ayant SAC activé — ce qui est le comportement par défaut sur de
nombreuses installations récentes de Windows 11. Signer les installateurs
(MSI et NSIS) publiés dans les releases est donc une condition de
distribution, pas une simple amélioration.

## SignPath, gratuit pour l'open source

[SignPath](https://signpath.org/) offre la signature de code gratuitement aux
projets open source via son programme OSS. La procédure :

1. Candidater sur la page du programme OSS de SignPath
   (voir https://signpath.org/ pour le lien d'inscription en cours).
2. Une fois accepté, SignPath fournit une organisation, un projet et une
   politique de signature (`organization-id`, `project-slug`,
   `signing-policy-slug`), ainsi qu'un jeton d'API (`api-token`) à créer
   depuis le portail SignPath.
3. Renseigner ces quatre valeurs comme secrets du dépôt GitHub (Settings →
   Secrets and variables → Actions → New repository secret) :
   - `SIGNPATH_API_TOKEN`
   - `SIGNPATH_ORGANIZATION_ID`
   - `SIGNPATH_PROJECT_SLUG`
   - `SIGNPATH_SIGNING_POLICY_SLUG`

## Utilisation dans le workflow de release

`.github/workflows/release.yml` vérifie la présence des quatre secrets
ci-dessus avant de soumettre une demande de signature. Si l'un des quatre
manque, l'étape de signature est simplement ignorée et la release publiée par
`tauri-apps/tauri-action` reste non signée, avec une note dans les notes de
version. Une fois les secrets renseignés, le workflow téléverse les
installateurs comme artefact de build, soumet une demande de signature via
`signpath/github-action-submit-signing-request`, attend la fin de la
signature (`wait-for-completion: true`) et récupère l'installateur signé.

## Alternative : Azure Trusted Signing

Azure Trusted Signing est une alternative payante à l'usage (pas de
certificat matériel à gérer) : elle demande un abonnement Azure, une identité
vérifiée par Microsoft, et l'action `azure/trusted-signing-action` dans le
workflow de release. Elle convient si le projet quitte un jour le périmètre
gratuit du programme OSS de SignPath (organisation à but commercial, volume
de signatures trop élevé, etc.).
