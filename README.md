<p align="center">
  <p align=center>
    <img src="./resources/icon.ico" alt="AniMathIO Logo"/>
  </p>
  <h1 align="center">AniMathIO</h1>
</p>

AniMathIO revolutionizes the creation of mathematical videos, tailored for educators, students, and professionals seeking to bring complex concepts to life

## Special thanks for these projects

They are core part of the AniMathIO software! Check them out!

- [mafs](https://github.com/stevenpetryk/mafs)
- [nextron](https://github.com/saltyshiomix/nextron)

## Table of Contents

- [Special thanks for these projects](#special-thanks-for-these-projects)
- [Table of Contents](#table-of-contents)
- [OS Support](#os-support)
- [Installation](#installation)
- [AI Agent Integration (MCP)](#ai-agent-integration-mcp)
- [Development build](#development-build)
  - [Canvas build fix](#canvas-build-fix)
    - [NOTE: If you want proper development you will need to follow the steps regarding the canvas build and install all the necessary dependencies](#note-if-you-want-proper-development-you-will-need-to-follow-the-steps-regarding-the-canvas-build-and-install-all-the-necessary-dependencies)
  - [Running the app](#running-the-app)
- [Contributing](#contributing)
- [Star History](#star-history)

## OS Support

We are supporting the following operating systems: Windows, Linux (via AppImage, Snap, Flatpak, DEB packages, and AUR).

**NOTE**: macOS is currently **unsupported**. Due to the lack of available resources, the latest version of AniMathIO that supports macOS is the legacy 1.3.0 version.

## Installation

### Windows

1. Download the latest `AniMathIO.Setup.X.X.X.exe` installer from the [release page](https://github.com/AniMathIO/AniMathIO/releases)
2. Run the installer and follow the installation wizard
3. Launch AniMathIO from the Start Menu or desktop shortcut

Alternatively, you can download `win-unpacked.zip` for a portable version (no installation required).

### Linux

AniMathIO is available in multiple formats for Linux distributions:

#### AppImage (Universal Linux)

1. Download `AniMathIO-X.X.X.AppImage` from the [release page](https://github.com/AniMathIO/AniMathIO/releases)
2. Make it executable:

   ```bash
   chmod +x AniMathIO-X.X.X.AppImage
   ```

3. Run it:

   ```bash
   ./AniMathIO-X.X.X.AppImage
   ```

#### Snap Package (Universal Linux)

Install via snap:

```bash
sudo snap install animathio
```

Or download the `.snap` file from the [release page](https://github.com/AniMathIO/AniMathIO/releases) and install it:

```bash
sudo snap install --dangerous animathio_X.X.X_amd64.snap
```

#### Flatpak (Experimental)

**Note**: Flatpak support is experimental and may have limitations.

1. Download `AniMathIO-X.X.X-x86_64.flatpak` from the [release page](https://github.com/AniMathIO/AniMathIO/releases)
2. Install it:

   ```bash
   flatpak install AniMathIO-X.X.X-x86_64.flatpak
   ```

#### DEB Package (Debian/Ubuntu-based)

1. Download `animathio_X.X.X_amd64.deb` from the [release page](https://github.com/AniMathIO/AniMathIO/releases)
2. Install it:

   ```bash
   sudo dpkg -i animathio_X.X.X_amd64.deb
   sudo apt-get install -f  # Install any missing dependencies
   ```

#### Arch Linux (AUR)

AniMathIO is available in the AUR as `animathio-bin`:

```bash
# Using yay (recommended)
yay -S animathio-bin

# Or using paru
paru -S animathio-bin

# Or manually with makepkg
git clone https://aur.archlinux.org/animathio-bin.git
cd animathio-bin
makepkg -si
```

**AUR Package**: [animathio-bin](https://aur.archlinux.org/packages/animathio-bin)

#### Portable Linux

Download `linux-unpacked.tar.gz` from the [release page](https://github.com/AniMathIO/AniMathIO/releases), extract it, and run the executable directly.

### macOS

⚠️ **macOS is currently unsupported**. The latest version of AniMathIO that supports macOS is the legacy 1.3.0 version, available on the [release page](https://github.com/AniMathIO/AniMathIO/releases).

---

**Alternative Download**: You can also download binaries from our website: [https://animathio.com/](https://animathio.com)

## AI Agent Integration (MCP)

AniMathIO includes a built-in [Model Context Protocol](https://modelcontextprotocol.io/) server, so AI agents such as Claude can drive the editor while you watch changes appear on the canvas.

The server runs inside the Electron main process, so AniMathIO must be running with a project open. It is **disabled by default** — enable it under **Settings**, which shows the endpoint and an auth token.

- **Endpoint:** `http://127.0.0.1:<port>/mcp` (default port `4517`, loopback only)
- **Auth:** bearer token, shown in Settings
- **Transport:** HTTP MCP, not stdio

### Available tools

| Tool | Purpose |
| --- | --- |
| `get_project_state` | Read canvas, timeline, elements and animations |
| `add_text` | Add a text element |
| `add_math` | Add a LaTeX fragment, typeset via KaTeX |
| `import_manim_scene` | Convert a whole Manim Community script into timeline elements |
| `add_animation` | Attach fadeIn/fadeOut/slideIn/slideOut/breathe/mafsReveal |
| `update_element` / `remove_element` | Change or delete an element |
| `set_canvas` | Canvas size, background colour, timeline duration |
| `seek` / `set_playing` | Move the playhead, start/stop playback |
| `save_project` | Save the open project |
| `add_media` | Import an image, video, or audio file from an absolute local path into the project |
| `export_video` | Render the project to an absolute `.mp4` or `.webm` path |

`import_manim_scene` is the highest-leverage one: it builds an entire scene in a single call.

`add_media` takes a required `path` on the machine running AniMathIO and returns the created element IDs; a video creates both video and audio elements. It selects the matching resource panel and waits for the media to decode. Unsupported file types, missing files, and decoding failures report an error.

`export_video` takes a required absolute `path` ending in `.mp4` or `.webm` and an optional `format` that must match the extension (otherwise the extension determines the format). The parent directory must exist; an existing destination is overwritten. Export records in **real time**: a 30-second project takes at least 30 seconds, plus conversion and writing. MP4 conversion fetches the FFmpeg core if needed. Allow a long client timeout, keep the project open, and avoid editing during export. The tool returns after the file is written.

> **Security:** a connected agent can read local media, modify the open project, and write files. Keep the token private, and enable the server only while you are using it.

### Testing the connection

```console
node scripts/mcp-smoke.mjs --token <token>                  # list tools
node scripts/mcp-smoke.mjs --token <token> --tool get_project_state
```

## Development build

Clone project

```console
git clone
```

Install dependencies

```console
cd AniMathIO
npm install
```

### Canvas build fix

Temporary fix for canvas build errors

```console
# if canvas errors occur regardig node mismatch run the following line
npm rebuild canvas --update-binary
```

#### NOTE: If you want proper development you will need to follow the steps regarding the canvas build and install all the necessary dependencies

- [All platforms](https://github.com/Automattic/node-canvas/wiki)
- [Windows](https://github.com/Automattic/node-canvas/wiki/Installation:-Windows)

### Running the app

```console
# development mode
npm run dev

# production build
npm run build
```

## Contributing

We welcome contributions to AniMathIO. Please read our [contributing guidelines](./CONTRIBUTING.md) to get started.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=animathio/animathio&type=Date)](https://star-history.com/#animathio/animathio&Date)
