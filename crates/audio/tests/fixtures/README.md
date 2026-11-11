Generated 80 ms, 1 kHz, mono 48 kHz sine fixtures (no third-party media).

Generation: `ffmpeg -f lavfi -i sine=frequency=1000:sample_rate=48000:duration=0.08 -threads 1 -c:a CODEC tone.EXT`

Codecs: WAV pcm_s16le, FLAC flac, MP3 libmp3lame, ADTS AAC/M4A aac,
Ogg Vorbis libvorbis, Ogg Opus libopus. Opus tests the explicit unsupported path
in the pinned Symphonia 0.5 adapter. Tests need no installed ffmpeg.
