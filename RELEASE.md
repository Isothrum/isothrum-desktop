# Releasing the Isothrum desktop app

The desktop app ships through `.github/workflows/release.yml`: push a version tag and the workflow
builds the installers for Linux, macOS and Windows on a **draft** GitHub release, with a
`latest.json` the app's auto-updater reads. You review the draft and publish it.

## Trigger

- **Tag:** `git tag v0.2.0 && git push origin v0.2.0`. The matrix builds every platform and
  attaches the installers (and `latest.json`) to a draft release named after the tag.
- **Manual:** run the *Release* workflow from the Actions tab (`workflow_dispatch`). It tags the
  draft from the version in `src-tauri/tauri.conf.json` (`v<version>`); no git tag is pushed.

Then open the draft release, check the assets, and publish it. Publishing is a human step — the
workflow never publishes on its own, and no release is cut by building.

## What a build produces

| Platform | Installers | Runner |
| --- | --- | --- |
| Linux | `.deb`, `.AppImage` | `ubuntu-22.04` |
| macOS | `.dmg`, one for Apple Silicon, one for Intel | `macos-latest` |
| Windows | `.msi` | `windows-latest` |

`latest.json` plus the per-platform update bundles and their `.sig` signatures are attached when
the updater key is set (see below). The updater endpoint is baked into `tauri.conf.json`
(`https://github.com/Isothrum/isothrum-desktop/releases/latest/download/latest.json`) and resolves
once the release is published (not a draft).

Beside the installers, the draft carries:

| Job | What |
| --- | --- |
| `sbom` | CycloneDX SBOMs: `isothrum-desktop-rust.cdx.json` (cargo-cyclonedx, every target) and `isothrum-desktop-npm.cdx.json` (`npm sbom`, without the build-only packages) |
| `attest` | `SHA256SUMS` of every asset, signed keylessly with cosign (`SHA256SUMS.sigstore.json`: the certificate names `release.yml` at the tag, recorded in Sigstore's public log, checked in the job); a build provenance attestation of each asset; an attestation of each SBOM of each installer |

A user checks a download as [README.md](README.md#verify-a-download) says. None of these needs a
secret: they sign with the workflow's own identity (OIDC, `id-token: write`).

## What each build signs

| Platform | Signature | Checked in the build |
| --- | --- | --- |
| Windows | Authenticode through Azure Artifact Signing (`trusted-signing-cli`, pinned): the app and the MSI | `Get-AuthenticodeSignature` is `Valid` on each |
| macOS | Developer ID Application certificate; the app notarized and stapled by tauri-action, each DMG notarized and stapled after (tag builds) | `codesign --verify --deep --strict`, `spctl -a` (app and DMG) |
| Updates | minisign with the updater key: the `.sig` beside each update bundle, and `latest.json` | by the installed app, against the public key in `tauri.conf.json` |
| Store | Microsoft signs the MSIX | |

A build signs what it has secrets for; on a tag, the `prepare` job names any signing secret that
isn't set, in a warning, before building.

## Secrets

Set these under **Settings → Secrets and variables → Actions**. None is printed or committed.

### Required for a working release

| Secret | Purpose |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The updater signing key (minisign). Signs each update bundle so the installed app accepts the update. Its **public** half is in `tauri.conf.json` under `plugins.updater.pubkey` and the two must match. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password set when the key was generated (empty string if the key has none — still set the secret). |

If `TAURI_SIGNING_PRIVATE_KEY` is absent the build still succeeds, but it produces **no** update
signatures or `latest.json`, so the auto-updater cannot verify an update. A build like that is fine
for manual testing of the installers, not for a release users auto-update to.

### Optional — macOS signing and notarization

Without these, macOS builds are **unsigned**: they install and run for testing, but Gatekeeper
warns on first open and the updater is not notarized.

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | Developer ID Application certificate, exported as a `.p12` and base64-encoded. |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password. |
| `APPLE_SIGNING_IDENTITY` | The certificate's name, e.g. `Developer ID Application: Your Name (TEAMID)`. |
| `APPLE_API_KEY_P8` | The App Store Connect API key used to notarize (`.p8`, base64-encoded). |
| `APPLE_API_KEY_ID` | That key's id. |
| `APPLE_API_ISSUER` | That key's issuer id. |

### Optional — Windows signing

Without these, Windows `.msi` builds are **unsigned** (SmartScreen warns on install). The workflow
signs through Azure Artifact Signing (`trusted-signing-cli`), as the Cyber CTF launcher does.

| Secret | Purpose |
| --- | --- |
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` | The Entra ID app allowed to sign with the Artifact Signing account. |
| `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE` | The Artifact Signing endpoint (e.g. `https://weu.codesigning.azure.net`), account and certificate profile. |

When `AZURE_CLIENT_ID` is set the app and the MSI are signed and their signatures checked; when it
is absent, signing is skipped.

## Generating the updater keypair

Once, on a trusted machine (the private key never enters the repository):

```
cargo tauri signer generate -w ~/.isothrum/updater.key
# or, without a cargo-tauri install, from apps/desktop:
npm run tauri -- signer generate -w ~/.isothrum/updater.key
```

This prints a **public key** and writes the **private key** to `~/.isothrum/updater.key`
(protected by the password you choose). Then:

1. Put the public key into `src-tauri/tauri.conf.json` at `plugins.updater.pubkey` (the value there
   must be this public key, or the installed app rejects every update).
2. Set `TAURI_SIGNING_PRIVATE_KEY` to the contents of `~/.isothrum/updater.key` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to its password.
3. Back up the private key and password offline. If the key is lost, installed apps can no longer
   auto-update and users must reinstall from a release signed with a new key.

## Building locally (no release)

```
npm install
npm run build        # runs `npm run editor`, then bundles .deb/.AppImage/.dmg/.msi for this OS
```

The editor comes from the `@isothrum/editor` package (`npm run editor` copies its prebuilt
bundle). A local bundle is unsigned unless you export the signing variables above into your shell. To skip
bundling (binary + editor only, when you lack signing secrets), run `cargo tauri build --no-bundle`
from `src-tauri`.
