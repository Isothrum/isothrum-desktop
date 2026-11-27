# Changelog

All notable changes to Isothrum desktop are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] — 2026-11-27

### Changed

- **Built on Isoloom 3.0 and Isothrum 3.0.** The app uses `isoloom-spec`,
  `isoloom-targets` and `isoloom-topology` 3.0.0. A machine's console opens in
  this computer's terminal through `isothrum-machine` and `isothrum-runner` 3.0.0.
- **A lab with VMs runs every machine as a VM**, from Isoloom's Vagrant output. A
  container machine runs in the VM form that its OS or its service gives it. A plain
  image has no VM form, so a lab that mixes a plain image with a VM-only machine
  doesn't run.
- **Traffic captures in a lab of VMs** run `tcpdump` in a VM on the segment, over
  `vagrant ssh`.
- **The editor can call exactly the app's 18 commands.** The commands the app
  registers, the permissions it makes and the editor's capability are checked to
  be the same list. The bundled editor's content security policy allows only its
  own scripts and WebAssembly: no inline scripts, no `eval`.
- **macOS builds per architecture**, Apple silicon and Intel, instead of one
  universal build.

### Removed

- **The hybrid output's Docker host VM** (`vms.rs`). A lab's containers no longer
  run inside a VM next to its VMs.

### Security

- **Each release carries ways to verify a download:**
  - CycloneDX SBOMs of the Rust crates and the npm packages.
  - `SHA256SUMS` of every asset, with a keyless Sigstore signature
    (`SHA256SUMS.sigstore.json`).
  - A build provenance attestation of each asset and an SBOM attestation of each
    installer.

  README.md ("Verify a download") shows how to check them.
- **Each build checks its own signatures:** Authenticode on Windows, and the
  code signature and Gatekeeper on macOS. Before any build, the release names the
  signing secrets that aren't set.
- **No build runs unless the check passes.** The release workflow runs `gate.yml`
  first, and its actions are pinned to commit SHAs.

### Development

- `npm run check` runs the editor build, rustfmt, clippy and the tests, which is
  what CI runs. `npm run check:real` runs the tests that need Docker or Vagrant.
- **CI:** `ci.yml` runs on pull requests into `dev`, and `gate.yml` before `main`,
  on Linux, macOS and Windows.
- **Tests:** shared helpers live in `test_support.rs`. The editor's commands are
  tested in `commands.rs`, as functions of the app's data folder.
- **Microsoft Store:** the submission script can create the first submission as a
  draft, for a person to finish in Partner Center.

## [1.0.0] — 2026-10-20

The first release.

[Unreleased]: https://github.com/Isothrum/isothrum-desktop/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/Isothrum/isothrum-desktop/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/Isothrum/isothrum-desktop/releases/tag/v1.0.0
