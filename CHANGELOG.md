# Changelog

All notable changes to AniMathIO 2.x are documented in this file. It is generated
from [Conventional Commits](https://www.conventionalcommits.org) by release-plz and
git-cliff. Changes to 1.x are in the [v1 release notes](https://github.com/AniMathIO/AniMathIO/releases?q=animathio-v1).

## [2.3.0](https://github.com/AniMathIO/AniMathIO/compare/v2.2.0...v2.3.0) - 2026-11-23

The last feature release of AniMathIO 2: from here on, security fixes and the dependency updates they need.

### Added

- *(ui)* A redesigned editor: neutral greys with one cyan accent, a flat inspector with one row layout for every clip kind, one dialog frame, grouped Preferences, a plain launcher, a shared tool-panel frame, plain plugin cards, a calmer timeline with single-row track headers and a tidier viewer ([#349](https://github.com/AniMathIO/AniMathIO/pull/349), [#351](https://github.com/AniMathIO/AniMathIO/pull/351), [#352](https://github.com/AniMathIO/AniMathIO/pull/352), [#353](https://github.com/AniMathIO/AniMathIO/pull/353), [#354](https://github.com/AniMathIO/AniMathIO/pull/354), [#355](https://github.com/AniMathIO/AniMathIO/pull/355))
- *(audio)* Record microphone audio into the Media library, optionally onto the timeline at the playhead ([#332](https://github.com/AniMathIO/AniMathIO/pull/332))
- *(assist)* Optional "Describe in words" LaTeX authoring with any OpenAI-compatible service or a local model; off until you set it up ([#317](https://github.com/AniMathIO/AniMathIO/pull/317))
- *(export)* WebM (VP9 + Opus) export alongside MP4 in the editor, CLI and MCP ([#338](https://github.com/AniMathIO/AniMathIO/pull/338))
- *(export)* Embed timeline markers as MP4 chapters ([#319](https://github.com/AniMathIO/AniMathIO/pull/319))
- Maintain pitch, above-1x video filtering and speed-ramp handles ([#314](https://github.com/AniMathIO/AniMathIO/pull/314))
- *(audio)* Learn the denoise profile from a range marker or selection, and a guided Auto duck panel ([#318](https://github.com/AniMathIO/AniMathIO/pull/318))
- *(animations)* Clipped and per-character slide presets via a slide effect (plugin ABI 1.3) ([#311](https://github.com/AniMathIO/AniMathIO/pull/311))
- Restore weighted text and 1.x typography presets ([#312](https://github.com/AniMathIO/AniMathIO/pull/312))

### Changed

- Dependency upgrades: wasmtime 51, symphonia 0.6, cpal 0.18, rubato 5, parley 0.13, wit-bindgen 0.63, gpui-component 0.10, toml 1 ([#368](https://github.com/AniMathIO/AniMathIO/pull/368)). Compiled plugins are recompiled once. Resampled audio is no longer byte-identical to 2.2 (differences below 4e-7 per sample), and assets with uncommon sample-rate ratios no longer lose about 3 ms at the start. Ogg Opus files decode to their exact length. Plugin authors need Rust 1.85 or newer.

### Fixed

- *(macos)* The bundle declares microphone usage, so recording asks for permission ([#360](https://github.com/AniMathIO/AniMathIO/pull/360))
- *(audio)* Undo or redo during a speed-key drag no longer crashes ([#360](https://github.com/AniMathIO/AniMathIO/pull/360))
- *(audio)* A recording that cannot be placed on the timeline stays in the library instead of being lost ([#360](https://github.com/AniMathIO/AniMathIO/pull/360))
- *(audio)* Recording: left and right channels can no longer swap, dropped audio is reported, Stop finishes off the UI thread and the clip lands where the playhead was when Record was pressed ([#345](https://github.com/AniMathIO/AniMathIO/pull/345))
- *(audio)* Playback and recording keep going after a short audio underrun; microphones are listed by stable device ids ([#368](https://github.com/AniMathIO/AniMathIO/pull/368))
- *(audio)* Alt-click no longer adds a speed key on clips too narrow for the speed lane or on frame holds, and variable-frame-rate video is blended by frame identity in speed ramps ([#331](https://github.com/AniMathIO/AniMathIO/pull/331))
- *(graph)* Tick labels sit beside the axis instead of on it, stay inside the clip at edge and off-screen axes, and digits with counters (0, 4, 6, 8, 9) render hollow ([#370](https://github.com/AniMathIO/AniMathIO/pull/370))
- *(captions)* Keep the word-highlight pill from swallowing the inter-word spaces ([#369](https://github.com/AniMathIO/AniMathIO/pull/369))
- *(animations)* Character-slide presets no longer refuse text with an empty or unknown caption style ([#336](https://github.com/AniMathIO/AniMathIO/pull/336))
- *(assist)* Bound network error text and keep the key status when a conversion is cancelled ([#335](https://github.com/AniMathIO/AniMathIO/pull/335))
- *(ui)* Right-to-left system locales no longer mirror half the editor; sliders no longer capture the timeline keys ([#368](https://github.com/AniMathIO/AniMathIO/pull/368))
- *(ui)* Inspector Fit labels, keyframe diamonds clear of clip labels, Enter confirms template dialogs, size presets switch to Custom on manual edits, and the transport timecode fits at 1024x768 ([#373](https://github.com/AniMathIO/AniMathIO/pull/373))
- *(editor)* First-time-user usability pass ([#321](https://github.com/AniMathIO/AniMathIO/pull/321))
- *(ui)* Keep key controls visible at 1280x720 (math dialog, launcher, Library dock) ([#322](https://github.com/AniMathIO/AniMathIO/pull/322))
- *(ui)* Final UI/UX review polish (audio gain, Auto duck, Math assist, settings, shape inspector, ruler) ([#323](https://github.com/AniMathIO/AniMathIO/pull/323))
- *(ui)* Polish community plugins, Preferences, Export and the launcher ([#324](https://github.com/AniMathIO/AniMathIO/pull/324))
- *(ui)* Inspector consistency, content-hugging selection box, track headers and 720p density ([#325](https://github.com/AniMathIO/AniMathIO/pull/325))

### Performance

- Cache ruler label widths, stretch fingerprints and viewer ink bounds; checked frame-hold math ([#342](https://github.com/AniMathIO/AniMathIO/pull/342))

## [2.2.0](https://github.com/AniMathIO/AniMathIO/compare/v2.1.0...v2.2.0) - 2026-11-17


### Added

- *(editor)* Reorganize the Library into Media, Elements, Titles, Effects and Plugins ([#307](https://github.com/AniMathIO/AniMathIO/pull/307))
- *(editor)* Browse and manage community plugins ([#309](https://github.com/AniMathIO/AniMathIO/pull/309))
- *(plugins)* Community plugin trust model, verified install, hot load and CLI ([#308](https://github.com/AniMathIO/AniMathIO/pull/308))
- *(latex)* Render inline LaTeX colours (`\color`, `\textcolor`) with an additive plugin ABI 1.2.0 ([#306](https://github.com/AniMathIO/AniMathIO/pull/306))
- *(package)* Offline Flatpak build of the desktop app, `com.animathio.AniMathIO` ([#297](https://github.com/AniMathIO/AniMathIO/pull/297))


## [2.1.0](https://github.com/AniMathIO/AniMathIO/compare/v2.0.0...v2.1.0) - 2026-11-16


### Added

- *(editor)* Notify when a newer release is available, at most once a day; opt out in Preferences or with `ANIMATHIO_NO_UPDATE_CHECK=1` ([#289](https://github.com/AniMathIO/AniMathIO/pull/289))

### Security

- Clear open dependency advisories ([#268](https://github.com/AniMathIO/AniMathIO/pull/268))

### Fixed

- Write JSON project assets atomically ([#264](https://github.com/AniMathIO/AniMathIO/pull/264))
- *(fuzz)* Media_probe treats guarded panics as clean errors ([#288](https://github.com/AniMathIO/AniMathIO/pull/288))
- Cap undo history and report worker spawn failures ([#263](https://github.com/AniMathIO/AniMathIO/pull/263))
- `\tfrac` text-style size and launcher status bar placeholder ([#271](https://github.com/AniMathIO/AniMathIO/pull/271))
- *(control)* Never hang on a wedged editor without breaking long jobs ([#274](https://github.com/AniMathIO/AniMathIO/pull/274))
- Refuse exports that would overwrite the open project file ([#272](https://github.com/AniMathIO/AniMathIO/pull/272))
- Control socket keeps pipelined requests, MCP fade-out/slide-out animate the clip end, MCP `add_text` is one undo step, v1 `breathe` import is bounded ([#275](https://github.com/AniMathIO/AniMathIO/pull/275))


## 2.0.0 - 2026-11-16

AniMathIO 2.0 is a from-scratch Rust rewrite of the editor: a small video editor core
(media clips, multi-track timeline, keyframes, audio, preview, export) with text, shapes,
effects, LaTeX, graphs, Manim and v1 import and MCP as sandboxed WebAssembly plugins or
sidecars. Projects use the new `.amio` format; open a 1.x project with
`animathio-cli import-v1` (see `docs/migrating-from-v1.md`).


### Security

- Harden plugin disk cache and control socket permissions ([#174](https://github.com/AniMathIO/AniMathIO/pull/174))
- Update zip to 9.0.5 (GHSA-m665-v7pm-7grj and related zip advisories) ([#266](https://github.com/AniMathIO/AniMathIO/pull/266))

### Added

- *(core)* Add v2 workspace, reversible model and project format ([#157](https://github.com/AniMathIO/AniMathIO/pull/157))
- *(plugin)* WebAssembly Component Model risk spike and findings ([#160](https://github.com/AniMathIO/AniMathIO/pull/160))
- *(audio)* Symphonia/rubato mixer, cpal/rtrb player and master clock ([#161](https://github.com/AniMathIO/AniMathIO/pull/161))
- *(render)* Deterministic Vello CPU renderer and headless PNG CLI ([#162](https://github.com/AniMathIO/AniMathIO/pull/162))
- *(editor)* Minimal GPUI editor shell (M3 start) ([#166](https://github.com/AniMathIO/AniMathIO/pull/166))
- *(import-v1)* One-way v1 .animathio project importer ([#167](https://github.com/AniMathIO/AniMathIO/pull/167))
- *(plugin)* Plugin-api, plugin-host, first-party plugins and sandbox tests ([#164](https://github.com/AniMathIO/AniMathIO/pull/164))
- *(editor)* Close deferred shell gaps (M3 editor polish) ([#168](https://github.com/AniMathIO/AniMathIO/pull/168))
- *(math)* Host typeset service, latex and graph plugins ([#170](https://github.com/AniMathIO/AniMathIO/pull/170))
- *(import-manim)* Manim Community scene importer (M4) ([#171](https://github.com/AniMathIO/AniMathIO/pull/171))
- *(editor)* Plugin integration, auto inspector, keyframes and settings ([#175](https://github.com/AniMathIO/AniMathIO/pull/175))
- *(render)* Render builtin clip effects ([#176](https://github.com/AniMathIO/AniMathIO/pull/176))
- *(core)* Animate typed plugin parameters across the editor ([#180](https://github.com/AniMathIO/AniMathIO/pull/180))
- *(import-manim)* Translate inline mobjects and VGroups ([#182](https://github.com/AniMathIO/AniMathIO/pull/182))
- *(editor)* Gpui-component migration + DaVinci-style NLE UI rework ([#183](https://github.com/AniMathIO/AniMathIO/pull/183))
- Video transitions (cross dissolve, dips, wipe) ([#185](https://github.com/AniMathIO/AniMathIO/pull/185))
- *(render)* GPU preview backend (Vello on wgpu) with CPU fallback ([#187](https://github.com/AniMathIO/AniMathIO/pull/187))
- *(export)* Create parents, refuse silent overwrites and write atomically ([#186](https://github.com/AniMathIO/AniMathIO/pull/186))
- *(ui)* Editor foundations (modules, actions, title bar, resizable layout, toasts) ([#192](https://github.com/AniMathIO/AniMathIO/pull/192))
- *(editor)* Add navigation tools and workflow dialogs ([#193](https://github.com/AniMathIO/AniMathIO/pull/193))
- *(editor)* Audible shuttle and export queue ([#196](https://github.com/AniMathIO/AniMathIO/pull/196))
- *(editor)* Multi-select, clipboard, track rename/reorder and linked ripple ([#199](https://github.com/AniMathIO/AniMathIO/pull/199))
- Import linked companion audio ([#200](https://github.com/AniMathIO/AniMathIO/pull/200))
- *(plugins)* Move text and shapes out of core ([#198](https://github.com/AniMathIO/AniMathIO/pull/198))
- *(ui)* Premium app frame, theme and start screen ([#201](https://github.com/AniMathIO/AniMathIO/pull/201))
- *(editor)* Premium timeline visuals and navigation ([#202](https://github.com/AniMathIO/AniMathIO/pull/202))
- *(ui)* Viewer gizmo, transport, inspector cards and media pool grid ([#204](https://github.com/AniMathIO/AniMathIO/pull/204))
- *(plugins)* Move clip effects into the effects-basic plugin ([#203](https://github.com/AniMathIO/AniMathIO/pull/203))
- *(titles)* Title preset library, Titles tab, MCP presets and social formats ([#205](https://github.com/AniMathIO/AniMathIO/pull/205))
- *(ui)* Viewer picking, degree rotation, effect warnings and per-tile filmstrip ([#206](https://github.com/AniMathIO/AniMathIO/pull/206))
- *(subtitles)* Local automatic subtitles with Whisper, SRT/WebVTT and editor dialog ([#213](https://github.com/AniMathIO/AniMathIO/pull/213))
- *(autocut)* Silence and filler autocuts with timeline review, CLI and MCP ([#215](https://github.com/AniMathIO/AniMathIO/pull/215))
- *(plugins)* Freeze the component ABI at 1.0.0 ([#216](https://github.com/AniMathIO/AniMathIO/pull/216))
- *(templates)* Add portable templates and headless creation ([#217](https://github.com/AniMathIO/AniMathIO/pull/217))
- Add offline auto-reframe for portrait video ([#219](https://github.com/AniMathIO/AniMathIO/pull/219))
- Finish deferrals from #211, #214, #215, #216 ([#220](https://github.com/AniMathIO/AniMathIO/pull/220))
- *(markers)* Timeline markers, schema 5 and beat/marker sync ([#221](https://github.com/AniMathIO/AniMathIO/pull/221))
- *(subtitles)* Animated CapCut-style caption styles with per-word timings ([#223](https://github.com/AniMathIO/AniMathIO/pull/223))
- *(templates)* Launcher cards, fill placeholders, save as template and MCP tools ([#225](https://github.com/AniMathIO/AniMathIO/pull/225))
- *(audio)* Streaming decode and resample, Opus, surround downmix ([#226](https://github.com/AniMathIO/AniMathIO/pull/226))
- *(inspector)* Editable video effects with keyframes, reorder and reset ([#227](https://github.com/AniMathIO/AniMathIO/pull/227))
- *(plugins)* Plugin API 1.1 with animation-preset and importer worlds ([#230](https://github.com/AniMathIO/AniMathIO/pull/230))
- *(audio)* Auto-ducking under speech and spectral-gating denoise ([#222](https://github.com/AniMathIO/AniMathIO/pull/222))
- *(media)* Per-clip fit modes (format schema 6) and bug-bash deferrals ([#236](https://github.com/AniMathIO/AniMathIO/pull/236))
- *(markers)* Finish source guards, ripple, selection and chapter export ([#237](https://github.com/AniMathIO/AniMathIO/pull/237))
- *(editor)* Add readable UI scaling and Premiere shortcuts ([#240](https://github.com/AniMathIO/AniMathIO/pull/240))
- Add clip speed, reverse, frame holds and ramps (schema 7) ([#241](https://github.com/AniMathIO/AniMathIO/pull/241))
- *(editor)* Add a typed math objects workflow ([#242](https://github.com/AniMathIO/AniMathIO/pull/242))
- *(showcase)* Add three reproducible math explainers ([#244](https://github.com/AniMathIO/AniMathIO/pull/244))
- *(audio)* Align denoise and finish caption editing controls ([#245](https://github.com/AniMathIO/AniMathIO/pull/245))
- *(editor)* Add Premiere rate stretch, slip, slide and rolling tools ([#246](https://github.com/AniMathIO/AniMathIO/pull/246))
- *(audio)* Learn denoise profiles and duck individual clips ([#247](https://github.com/AniMathIO/AniMathIO/pull/247))
- Add reviewed scene detection cuts ([#251](https://github.com/AniMathIO/AniMathIO/pull/251))
- *(editor)* Open large and remember the window size ([#255](https://github.com/AniMathIO/AniMathIO/pull/255))

### Fixed

- *(audio)* Bound decode and resample allocations from untrusted media ([#163](https://github.com/AniMathIO/AniMathIO/pull/163))
- *(import-manim)* Confine ImageMobject path resolution ([#173](https://github.com/AniMathIO/AniMathIO/pull/173))
- Address project save, animation, export and importer audit defects ([#188](https://github.com/AniMathIO/AniMathIO/pull/188))
- Keep control connections and editor workers responsive ([#191](https://github.com/AniMathIO/AniMathIO/pull/191))
- *(cli)* Make media imports undoable and audio fades editable ([#195](https://github.com/AniMathIO/AniMathIO/pull/195))
- *(editor)* Pasted clips get their own link groups
- *(render)* Centre plain text on its clip position; preview passes failing effects through ([#212](https://github.com/AniMathIO/AniMathIO/pull/212))
- *(render)* Count every preview effect probe against one per-frame budget
- *(subtitles)* Bound subtitle timestamps and import offsets against overflow
- Close audit findings and complete audio effect controls ([#214](https://github.com/AniMathIO/AniMathIO/pull/214))
- Pre-MVP editor bug-bash (locks/undo, linked edits, unsaved-work guard, timeline and inspector fixes) ([#224](https://github.com/AniMathIO/AniMathIO/pull/224))
- *(mcp)* Validate create_from_template media and output paths like sibling tools ([#229](https://github.com/AniMathIO/AniMathIO/pull/229))
- Restore #218, #219 and #220 changes reverted by the #221 squash ([#228](https://github.com/AniMathIO/AniMathIO/pull/228))
- *(import-manim)* Bound every recursive parser production ([#232](https://github.com/AniMathIO/AniMathIO/pull/232))
- *(import-manim)* Bound repeated async prefixes in the parser ([#233](https://github.com/AniMathIO/AniMathIO/pull/233))
- *(editor)* Playback from the playhead, context-menu shortcuts, free-layer element placement, math and graph fixes ([#234](https://github.com/AniMathIO/AniMathIO/pull/234))
- *(brand)* Restore the original AniMathIO logo ([#235](https://github.com/AniMathIO/AniMathIO/pull/235))
- *(plugin-host)* Confine importer sibling reads with openat O_NOFOLLOW ([#239](https://github.com/AniMathIO/AniMathIO/pull/239))
- *(text)* Preserve glyph counters in text, titles and captions ([#248](https://github.com/AniMathIO/AniMathIO/pull/248))
- *(editor)* Finish the second hands-on usability pass ([#250](https://github.com/AniMathIO/AniMathIO/pull/250))
- Harden Windows sibling reads and clip graph plots ([#249](https://github.com/AniMathIO/AniMathIO/pull/249))
- Reject malformed projects and media found by fuzzing ([#254](https://github.com/AniMathIO/AniMathIO/pull/254))
- Prevent dangling asset references when removing and saving ([#256](https://github.com/AniMathIO/AniMathIO/pull/256))
- *(editor)* Finish track order, dialog and canvas usability ([#257](https://github.com/AniMathIO/AniMathIO/pull/257))
- *(latex)* Restore math rules and preserve counters during Write on ([#259](https://github.com/AniMathIO/AniMathIO/pull/259))
- *(latex)* Embed MiTeX helpers for everyday commands ([#261](https://github.com/AniMathIO/AniMathIO/pull/261))
- Resolve plugin discovery, typesetting stack and project backgrounds ([#260](https://github.com/AniMathIO/AniMathIO/pull/260))

### Performance

- *(export)* Measure 1080p export, table-driven colour conversion, stage timers ([#208](https://github.com/AniMathIO/AniMathIO/pull/208))
- *(bench)* Measure v1 versus v2 startup, memory and export ([#258](https://github.com/AniMathIO/AniMathIO/pull/258))

