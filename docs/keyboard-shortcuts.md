# Keyboard shortcuts

AniMathIO 2.0 defaults to Adobe Premiere Pro's Windows/Linux shortcuts, with
Ctrl mapped to Cmd (`secondary`) on macOS, following [Adobe's official default
list](https://helpx.adobe.com/premiere-pro/using/keyboard-shortcuts.html).
Help → Keyboard shortcuts (Ctrl+Alt+K) opens a searchable dialog over the same
table. Panel shortcuts pause while a text input has focus; menu bar, context
menus and tooltips resolve their hints from the same action bindings. Actions
Premiere does not have keep a sensible non-conflicting key.

## Tools (Timeline)

| Action | Shortcut |
| --- | --- |
| SelectTool | `v` |
| BladeTool (razor) | `c` |
| RippleTool (ripple edit) | `b` |
| HandTool | `h` |

## Edits

| Action | Shortcut |
| --- | --- |
| SplitClip (Add Edit) | `Ctrl-k` |
| SplitAllTracks (Add Edit to All Tracks) | `Ctrl-shift-k` |
| RippleTrimPrevious (Q: ripple-trim previous edit to playhead) | `q` |
| RippleTrimNext (W: ripple-trim next edit to playhead) | `w` |
| RippleDelete | `shift-delete`, `alt-backspace` |

Q/W act on the selected clip's track (the "targeted" track), else the track
under the playhead: inside a clip the edge under the playhead ripple-trims; in a
gap the segment between the playhead and the neighbouring edit closes, sliding
later clips on that track left. Markers follow the ripple setting.

## In / Out and markers

| Action | Shortcut |
| --- | --- |
| MarkIn | `i` |
| MarkOut | `o` |
| GoToIn | `shift-i` |
| GoToOut | `shift-o` |
| MarkClip | `x` |
| ClearMarks (Clear In and Out) | `Ctrl-shift-x` |
| AddMarker | `m` |
| NextMarker | `shift-m` |
| PreviousMarker | `Ctrl-shift-m` |

## Playback and navigation

| Action | Shortcut |
| --- | --- |
| ShuttleReverse | `j` |
| ShuttleStop | `k` |
| ShuttleForward | `l` |
| PreviousEdit | `up` |
| NextEdit | `down` |
| PreviousFrame | `left` |
| NextFrame | `right` |
| BackFiveFrames | `shift-left` |
| ForwardFiveFrames | `shift-right` |
| GoToStart | `home` |
| GoToEnd | `end` |
| TogglePlayback | `space` |
| ToggleLoop | `alt-l` |

## Zoom and panels

| Action | Shortcut |
| --- | --- |
| ZoomIn (timeline) | `=` |
| ZoomOut (timeline) | `-` |
| ZoomToFit (timeline) | `\` |
| LargerTracks | `Ctrl-alt-=` |
| SmallerTracks | `Ctrl-alt--` |
| FocusLibrary (Premiere's Projects panel) | `shift-1` |
| FocusTimeline | `shift-3` |
| FocusViewer (Program Monitor) | `shift-4` |
| FocusInspector (Effect Controls) | `shift-5` |
| FocusEffects | `shift-7` |
| FitViewer | `shift-f` |

## Transitions, clips and selection

| Action | Shortcut |
| --- | --- |
| DefaultVideoTransition (Ctrl+D family: 1 s fades, both edges) | `Ctrl-d` |
| DefaultAudioTransition (audio crossfade) | `Ctrl-shift-d` |
| DefaultTransitions (both kinds) | `shift-d` |
| UnlinkClips (Link / Group: link or unlink the selection) | `Ctrl-l`, `Ctrl-g` |
| ToggleSnapping | `s` |
| SelectAllClips | `Ctrl-a` |
| ClearSelection | `escape` |
| CopyClips | `Ctrl-c` |
| CutClips | `Ctrl-x` |
| PasteClips | `Ctrl-v` |
| DuplicateClips | `Ctrl-shift-/` |
| DeleteSelection | `delete`, `backspace` |
| NudgeLeft / NudgeRight | `alt-left` / `alt-right` |

## Project, files and help

| Action | Shortcut |
| --- | --- |
| NewProject | `Ctrl-alt-n` |
| OpenProject | `Ctrl-o` |
| SaveProject | `Ctrl-s` |
| ImportMedia | `Ctrl-i` |
| ExportVideo (Export Media) | `Ctrl-m` |
| Preferences | `Ctrl-,` |
| Undo | `Ctrl-z` |
| Redo | `Ctrl-shift-z` |
| KeyboardShortcuts | `Ctrl-alt-k` |

Left/Right always step the playhead one frame; Shift+Left/Right step five.
Alt+L keeps AniMathIO's loop shortcut (Premiere reuses Ctrl+L for Loop, which
here is Link). Mark Clip sets in/out from the first clip under the playhead in
track order; the marks are editor state.

Deferred: rolling-edit, rate-stretch, slip, slide, zoom and track-select tools
(we have no such tools yet); paste attributes and paste insert; lift/extract;
maximize panel; match frame; a "classic AniMathIO" preset and user keymap
files. Actions without a default shortcut stay available in menus, toolbars and
toolbars. The searchable dialog lists actions with assigned keys.
