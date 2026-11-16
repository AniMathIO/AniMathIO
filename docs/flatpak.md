# Flatpak

## Install

Flatpak packaging is available for local builds and CI artifacts; AniMathIO is not
published on Flathub yet. Install the `AniMathIO.flatpak` artifact from the Flatpak
workflow with `flatpak install --user AniMathIO.flatpak`, then run
`flatpak run com.animathio.AniMathIO`. Updates will come through Flatpak once a
store remote is configured.

## Build

The manifest is `packaging/flatpak/com.animathio.AniMathIO.yml`. From the repository root:

```sh
flatpak-builder --user --install-deps-from=flathub --force-clean build-dir packaging/flatpak/com.animathio.AniMathIO.yml
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/com.animathio.AniMathIO.yml
```

Freedesktop 26.08 supplies native libraries and the Rust SDK extension. The
extension does not include the WASI target: a checksum-pinned Rust 1.98.0
`wasm32-wasip2` standard-library archive is installed into a temporary writable
sysroot. The manifest checks the compiler version before using it; update both
when the SDK Rust version changes. `ANIMATHIO_SKIP_RUSTUP=1` tells
`scripts/build-plugins.sh` to use this offline target. The build sandbox has no
network access; Flatpak downloads the declared sources before building.

To refresh `cargo-sources.json`, install `flatpak-cargo-generator==0.1.5` and
`tomlkit` in a Python virtual environment, then run
`python packaging/flatpak/generate-sources.py` with that environment active.
The generator combines the main lockfile, every plugin lockfile and the hello
example lockfile before generating a single vendor configuration. Regenerate
whenever any of those lockfiles changes. The source directory is the current
checkout; Flathub submission must replace it with a pinned release source.

The build installs both binaries and eight plugin bundles under
`/app/lib/animathio/plugins`, which executable-relative discovery already trusts.
This initial package omits the optional whisper engine. It includes the core
subtitle import/editing functionality, but model-backed transcription is unavailable.

## Sandbox and release checks

Wayland, fallback X11, shared IPC, GPU access and PulseAudio are allowed. File
open/save dialogs use the existing rfd XDG portal backend: no blanket home,
Documents or Videos permission is granted. A working desktop portal implementation
is required. Select media through the portal; drag-and-drop and sibling-file
imports may require granting the containing directory explicitly. Test project
save/reopen, media import, export and Manim sibling reads on the target desktop
before submission. No GUI or sound is needed to build the package.

There is no network permission. `ANIMATHIO_NO_UPDATE_CHECK=1` disables the GitHub
update check because store updates should manage this package. Network-dependent
features are unavailable in this sandbox.

The maintainer must submit the manifest to Flathub, demonstrate control of
animathio.com for the `com.animathio.AniMathIO` ID, replace the local directory
source with a pinned release commit/archive, confirm metadata and screenshot
URLs, complete Flathub review and perform portal/GPU/audio smoke tests. CI builds
a bundle but does not submit or publish it. Part of #283.
