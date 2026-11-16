# Changelog

All notable changes to AniMathIO 2.x are documented in this file. It is generated
from [Conventional Commits](https://www.conventionalcommits.org) by release-plz and
git-cliff. Changes to 1.x are in the [v1 release notes](https://github.com/AniMathIO/AniMathIO/releases?q=animathio-v1).

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

