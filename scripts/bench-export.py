#!/usr/bin/env python3
"""Generate a deterministic 1080p30, 10s edit and benchmark the release CLI.
Requires ffmpeg, built text/effects-basic bundles and target/release/animathio-cli.
"""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import zipfile

out = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/animathio-export-bench')
out.mkdir(parents=True, exist_ok=True)
cli = Path('target/release/animathio-cli').resolve()
subprocess.run(['nice', '-n', '19', 'ffmpeg', '-v', 'error', '-y', '-f', 'lavfi', '-i',
                'testsrc2=size=1920x1080:rate=30:duration=11', '-f', 'lavfi', '-i',
                'sine=frequency=440:sample_rate=48000:duration=11', '-c:v', 'libx264',
                '-threads', '3', '-preset', 'fast', '-pix_fmt', 'yuv420p', '-bf', '0',
                '-g', '30', '-c:a', 'aac', str(out / 'source.mp4')], check=True)
subprocess.run([cli, 'import-media', str(out / 'source.mp4'), '-o', str(out / 'source.amio')], check=True)
with zipfile.ZipFile(out / 'source.amio') as z:
    files = {name: z.read(name) for name in z.namelist()}
name = next(n for n in files if n.endswith('.json'))
p = json.loads(files[name])
ticks = 705600000
p.update(width=1920, height=1080, duration=10*ticks, frame_rate={'numerator': 30, 'denominator': 1})
v = p['tracks'][0]
c = next(iter(v['clips'].values()))
c['link_group'] = None
c['range'] = {'start': 0, 'end': 5*ticks}
c['effects'] = [{'id': 1, 'plugin': 'org.animathio.effects-basic', 'effect': 'grayscale', 'params': {'amount': 0.35}}]
d = copy.deepcopy(c)
d.update(id=3, range={'start': 5*ticks, 'end': 10*ticks}, source_offset=5*ticks)
v['clips']['3'] = d
v['transitions'] = [{'id': 1, 'kind': 'CrossDissolve', 'duration': ticks, 'from': c['id'], 'to': 3}]
for a in p['tracks'][1]['clips'].values():
    a['range']['end'] = 10*ticks
    a['link_group'] = None
track = copy.deepcopy(p['tracks'][1])
title = copy.deepcopy(c)
title.update(id=4, kind={'Plugin': {'plugin_id': 'org.animathio.text', 'params': {'text': '1080p export benchmark', 'font_size': 0.06, 'color': [255,255,255,255], 'units': 'normalized', 'align': 'left'}}}, range={'start': 0, 'end': 10*ticks}, effects=[])
track.update(id=3, name='Title', clips={'4': title})
p['tracks'].append(track)
files[name] = json.dumps(p).encode()
with zipfile.ZipFile(out / 'bench.amio', 'w', zipfile.ZIP_DEFLATED) as z:
    for name, data in files.items():
        z.writestr(name, data)
env = dict(os.environ, ANIMATHIO_EXPORT_TIMING='1', ANIMATHIO_PLUGIN_DIR=str(Path('target/plugins').resolve()))
subprocess.run(['nice', '-n', '19', 'ionice', '-c3', cli, 'export', str(out / 'bench.amio'), '-o', str(out / 'bench.mp4'), '--overwrite', '--bench'], env=env, check=True)
