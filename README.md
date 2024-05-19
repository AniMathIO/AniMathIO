# AniMathIO

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

## Canvas build fix

Temporary fix for canvas build errors

```console
# if canvas errors occur regardig node mismatch run the following line
npm rebuild canvas --update-binary
```

### NOTE: If you want proper development you will need to follow the steps regarding the canvas build and install all the necessary dependencies

- [All platforms](https://github.com/Automattic/node-canvas/wiki)
- [Windows](https://github.com/Automattic/node-canvas/wiki/Installation:-Windows)

## Running the app

```console
# development mode
npm run dev

# production build
npm run build
```
