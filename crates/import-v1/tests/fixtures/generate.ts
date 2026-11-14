// Run with tsx from an extracted v1 tree; see README.md.
import { translateManimScene } from './renderer/utils/manim/translator';
import { ProjectStore } from './renderer/states/stores/ProjectStore';
import { readFileSync, writeFileSync } from 'node:fs';
import { inflateSync } from 'node:zlib';
const out = process.argv[2];
// Browser FileReader equivalent for Node's Blob (serializer uses only this API).
(globalThis as any).FileReader = class {
  result: string; onloadend: () => void;
  async readAsDataURL(blob: Blob) {
    this.result = `data:${blob.type};base64,${Buffer.from(await blob.arrayBuffer()).toString('base64')}`;
    this.onloadend();
  }
};
const url = (mime: string, file: string) => `data:${mime};base64,${readFileSync(file).toString('base64')}`;
const png = url('image/png', 'tile.png');
const video = url('video/mp4', 'tile.mp4');
const audio = url('audio/wav', 'tone.wav');
const element = (id: string, type: string, properties: any, i = 0) => ({
  id, type, name: id, konvaNode: { mustNotSurvive: true },
  placement: { x: i * 25, y: i * 10, width: 80, height: 50, rotation: 0, scaleX: 0.75, scaleY: 1.25 },
  timeFrame: { start: 0, end: 3000 }, properties,
});
async function save(name: string, elements: any[], animations: any[] = [], maxTime = 4000) {
  const root: any = {
    elementStore: { editorElements: elements }, canvasStore: { backgroundColor: '#112233', canvas_width: 320, canvas_height: 180 },
    selectedMenuOption: 'Text', mediaStore: { audios: [], videos: [], images: [] },
    playbackStore: { maxTime, currentKeyFrame: 0, fps: 30 }, animationStore: { animations }, exportStore: { selectedVideoFormat: 'mp4' },
  };
  const bytes = Buffer.from(await new ProjectStore(root).serialize());
  const json = inflateSync(bytes);
  if (JSON.parse(json.toString()).editorElements.some((e: any) => 'konvaNode' in e)) throw Error('serializer failed');
  writeFileSync(`${out}/${name}.animathio`, bytes);
  writeFileSync(`${out}/${name}.json`, json.toString() + '\n');
}
async function main() {
  const translated = await translateManimScene({ className: 'Fixture',
    mobjects: { title: { kind: 'Text', id: 'title', text: 'Manim → v1', color: '#ffffff' } },
    statements: [{ kind: 'play', animations: [{ animationType: 'Write', targetId: 'title' }] }, { kind: 'wait', duration: 2 }],
  }, { width: 320, height: 180 });
  await save('manim-translated', translated.elements, translated.animations);
  await save('empty', []);
  await save('unicode-styles', [element('unicode', 'text', { text: 'Árvíztűrő π ∑ 日本語 🚀', fontSize: 24, fontWeight: 700, color: '#ffeecc', splittedTexts: [] })]);
  const effects = ['none', 'blackAndWhite', 'sepia', 'invert', 'saturate'];
  const elements = effects.map((type, i) => element(`image-${type}`, 'image', { src: png, elementId: `img-${i}`, effect: { type } }, i));
  elements.push(element('video', 'video', { src: video, elementId: 'video', effect: { type: 'none' }, muted: true }));
  elements.push(element('audio', 'audio', { src: audio, elementId: 'audio', masterVolume: 0.8 }));
  elements.push(element('graph-snapshot', 'mafs', { src: png, elementId: 'graph', effect: { type: 'none' } }));
  const animations = ['fadeIn', 'fadeOut', 'slideIn', 'slideOut', 'breathe', 'mafsReveal'].map((type, i) => ({ id: `a${i}`, targetId: elements[i].id, type, duration: 500, properties: { direction: type === 'mafsReveal' ? 'in' : 'left', textType: 'character', useClipPath: true } }));
  await save('media-effects-animations', elements, animations);
  await save('missing-media', [element('missing', 'image', { src: 'missing-image.png', elementId: 'missing', effect: { type: 'none' } })]);
  await save('huge-duration', [], [], 1e12);
}
main();
