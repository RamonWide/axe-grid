# Releasing

How a version of Axe-Grid gets from `main` to a download. The work is done by
[`.github/workflows/release.yml`](../.github/workflows/release.yml); a maintainer starts it and
reviews the result.

## What a release contains

| File | Built on | Contents |
|---|---|---|
| `axe-grid-<ver>-windows-x64.zip` | `windows-latest` | `axegrid.exe`, `axegrid-cli.exe`, `axegrid-setup.exe` (the installer: copies the programs into the user's account and adds desktop and Start-menu shortcuts), licence, attribution, install guide |
| `axe-grid-<ver>-macos-universal.zip` | `macos-latest` | `Axe-Grid.app` (arm64 + x86_64, ad-hoc signed), `axegrid-cli`, licence, attribution, install guide |
| `axe-grid-<ver>-linux-x86_64.tar.gz` | `ubuntu-22.04` | `axegrid`, `axegrid-cli`, desktop entry, icon, `install-desktop-entry.sh`, licence, attribution, install guide |
| `axe-grid-<ver>-linux-aarch64.tar.gz` | `ubuntu-22.04-arm` | the same, for 64-bit ARM |
| `SHA256SUMS.txt` | | a checksum for each file above |

Every file also gets a GitHub build provenance attestation, which ties it to the commit and
workflow run that produced it.

The builds are **not** code-signed for Windows or notarized for macOS, so both systems warn on
first launch; [`install.md`](install.md) tells users what to click. Signing needs a paid
certificate (Windows) and an Apple Developer ID (macOS); add them to the workflow when the project
has them.

## Steps

1. Make sure `main` is green in CI.
2. Choose the version. Below 1.0: a new minor version for new features or any change to command
   ids or the control protocol, a patch version for fixes only.
3. In one pull request:
   - set `version` under `[workspace.package]` in `Cargo.toml` and run `cargo build` so that
     `Cargo.lock` follows;
   - in `CHANGELOG.md`, rename "Unreleased" to the version with today's date, start a new empty
     "Unreleased", and update the links at the bottom;
   - update `ROADMAP.md` (the "Last reviewed" line and any number that moved) and the command and
     test counts in `README.md`.
4. Merge it, then tag that commit and push the tag:

   ```sh
   git tag -s v0.2.0 -m "Axe-Grid 0.2.0"
   git push origin v0.2.0
   ```

   The workflow refuses to run if the tag does not match the version in `Cargo.toml`.
5. The workflow builds every file and creates a **draft** release. Download at least the build for
   your own system and check that it starts, connects to the simulator, and, if you have a unit,
   connects to it.
6. Edit the draft's notes (the changelog section is a good start), then publish it.

## If something goes wrong

A draft release can be deleted and the tag moved while nobody has downloaded anything. Once a
release is published, do not replace its files: fix the problem and publish a new patch version.

## Trying the packaging without a release

Run the workflow by hand from the Actions tab ("Run workflow"). It builds and uploads the files as
workflow artifacts and skips creating a release.
