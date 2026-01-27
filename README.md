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
