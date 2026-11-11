# Rendering fixtures

- `sample.amio`: the command-built `animathio-cli new-sample` demo, with a PNG
  image, text, rectangle, ellipse, cubic/quadratic path, strokes and keyframes.
- `media.json` plus `assets/`: video and missing-plugin placeholders with rotated,
  scaled layers at 30000/1001 fps. The `.video` asset deliberately contains only a
  marker: the placeholder FrameSource does not decode media.

Regenerate the sample with the resource-limited Cargo command from AGENTS.md,
then regenerate PNGs with `UPDATE_GOLDEN=1` and inspect changes before committing.
The fixture projects remain committed independently of the sample builder so a
code change cannot silently redefine the reference inputs.
