Generated 80 ms, 1 kHz, mono 48 kHz sine fixtures (no third-party media).

Generation: `ffmpeg -f lavfi -i sine=frequency=1000:sample_rate=48000:duration=0.08 -threads 1 -c:a CODEC tone.EXT`

Codecs: WAV pcm_s16le, FLAC flac, MP3 libmp3lame, ADTS AAC/M4A aac,
Ogg Vorbis libvorbis, Ogg Opus libopus. Tests need no installed ffmpeg.

`long.*` (2.5 s chirp plus 1.5 kHz tone, 44.1 kHz; FLAC mono, the rest stereo) have enough
frames for several decode segments: `ffmpeg -f lavfi -i "aevalsrc=0.3*sin(2*PI*(200+300*t)*t)+0.1*sin(2*PI*1500*t)|0.25*sin(2*PI*(400+200*t)*t):s=44100:d=2.5"`
with `-c:a flac|libmp3lame -b:a 96k|aac -b:a 64k|libvorbis -q:a 2|libopus -b:a 64k`.
`opus_stereo.mka` / `opus_stereo.mp4` are 1 s of the same stereo signal at 48 kHz, `-c:a libopus
-b:a 48k` in Matroska and MP4, for the non-Ogg Opus paths.
