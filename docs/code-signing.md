# Code signing

## Why sign

Windows 11 with Smart App Control (SAC) enabled blocks the execution of any
locally compiled or downloaded binary that is not signed by a trusted
publisher. Without a signature, WinCleaner simply does not launch for a user
with SAC enabled — which is the default on many recent Windows 11
installations. Signing the installers (MSI and NSIS) published in the releases
is therefore a condition of distribution, not a nice-to-have.

## SignPath, free for open source

[SignPath](https://signpath.org/) offers code signing free of charge to open
source projects through its OSS programme. The procedure:

1. Apply on the SignPath OSS programme page (see https://signpath.org/ for the
   current sign-up link).
2. Once accepted, SignPath provides an organization, a project and a signing
   policy (`organization-id`, `project-slug`, `signing-policy-slug`), plus an
   API token (`api-token`) to create from the SignPath portal.
3. Store those four values as GitHub repository secrets (Settings → Secrets and
   variables → Actions → New repository secret):
   - `SIGNPATH_API_TOKEN`
   - `SIGNPATH_ORGANIZATION_ID`
   - `SIGNPATH_PROJECT_SLUG`
   - `SIGNPATH_SIGNING_POLICY_SLUG`

## Use in the release workflow

`.github/workflows/release.yml` checks that the four secrets above are present
before submitting a signing request. If any of the four is missing, the signing
step is simply skipped and the release published by `tauri-apps/tauri-action`
stays unsigned, with a note in the release body. Once the secrets are set, the
workflow uploads the installers as a build artifact, submits a signing request
through `signpath/github-action-submit-signing-request`, waits for the signing
to complete (`wait-for-completion: true`) and retrieves the signed installer.

## Alternative: Azure Trusted Signing

Azure Trusted Signing is a pay-per-use alternative (no hardware certificate to
manage): it requires an Azure subscription, an identity verified by Microsoft,
and the `azure/trusted-signing-action` action in the release workflow. It fits
if the project ever leaves the free scope of the SignPath OSS programme
(commercial organization, signing volume too high, and so on).
