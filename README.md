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

We are supporting the following operating systems: macOS, Windows, Linux (via universal dmg, exe, AppImage, Snap and unpacked).

**NOTE**: Help is needed for MacOS support.
Due to the lack of available resources, the latest version of AniMathIO that supports MacOS is the legacy 1.3.0 version.

## Installation

Please download the latest version of the project binaries from the [release page](https://github.com/AniMathIO/AniMathIO/releases)

Or from our website: [https://animathio.com/](https://animathio.com)

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
