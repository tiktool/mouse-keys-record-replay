# Listings: ready-to-paste drafts

Product: Mouse Keys Record Replay
Site: https://mouserec.com
Repo: https://github.com/tiktool/mouse-keys-record-replay (MIT)
Publisher: B4Battle LLC

Status: DRAFTS ONLY. Nothing here has been submitted or posted.

## 0. Gates before any submission

1. PLAYBACK SPEED COPY NEEDS v0.1.3 LIVE. Every block below that mentions playback speed (marked `[v0.1.3]`) describes a feature that is not in the released app yet. At commit 7224643 (v0.1.1) there is no speed code in `src/`, and the `feat/playback-speed` worktree (D:/Works/reprise-speed) had no speed code either when this kit was written. Do not post any `[v0.1.3]` line until the v0.1.3 release is published on GitHub and the slider and the F5 to F10 shortcuts have been checked on all three systems. To post before then, delete the `[v0.1.3]` lines.
2. mouserec.com: on 2026-11-09 `/` returned 200 and every sub-page did not. `/download`, `/features`, `/how-it-works`, `/faq`, `/compare`, `/windows`, `/macos`, `/linux` and all locale paths returned 404, because the full site (D:/Works/mouserec-site) is built but `deploy.sh` has not been run. Listings that link to sub-pages wait for that deploy.
2a. RECORDING HISTORY COPY NEEDS v0.1.4 LIVE. v0.1.4 adds an F4 panel with a list of past recordings. It is not in the released app and was not checked in code for this kit. Until v0.1.4 is published and checked on all three systems, the app keeps one recording, and every "no editor" limit line below stays true as written. Once v0.1.4 is live, add `[v0.1.4] Recording history: F4 opens a list of past recordings` next to each line tagged `(see gate 2a)`, and keep "no editor" (history is a list, not an editor).
2b. mouserec.com must pass the site stream's live checks before anything here is posted.
3. Screenshots: directories want real PNG screenshots (tray menu, status card while recording, settings window, loop card). The repo has `assets/demo.svg` and `assets/icon.png` but no screenshots. Capture them from the released build.

## 1. Facts sheet (every listing draws from this; each line checked against code)

| Fact | Evidence |
| --- | --- |
| Record: Ctrl+Alt+Shift+F1 (macOS Cmd+Option+Shift+F1), press again to stop | src/shortcuts.rs:20-30 |
| Play once: Ctrl+Alt+Shift+F2 (macOS Cmd+Option+Shift+F2) | src/shortcuts.rs:22,28 |
| Loop: Ctrl+Alt+Shift+F3 (macOS Cmd+Option+Shift+F3) | src/shortcuts.rs:24,30 |
| Shortcuts can be changed in Settings | README Features; src/shortcuts.rs parser |
| Any real mouse or keyboard input stops playback | src/engine.rs:91-102 (`on_input` sets ABORT while playing; injected events are ignored) |
| Synthetic events are tagged so playback does not stop itself | src/input/windows.rs:14-16 (dwExtraInfo), src/input/macos.rs:144,233 (EVENT_SOURCE_USER_DATA), src/input/linux.rs:55-62,116 (XTEST device ids) |
| Random timing toggle: each press moves 0.1 to 0.5 s earlier or later, positions, keys, hold lengths and order unchanged | src/engine.rs:53-55, 157-200 |
| Keys and buttons still held are released when playback ends or is stopped | src/engine.rs:455-466 |
| Updates verified with Ed25519 before install | src/update.rs:1-14, 110-113, 153 |
| Update check every 12 hours while running, can be turned off | src/update.rs:16; README Privacy |
| Network use: the update check against GitHub releases (api.github.com) and the release download | grep of src/ for URLs and Command::new: all network calls are in src/update.rs |
| Size: Windows exe 777,728 bytes; macOS zip 1,136,219 bytes; Linux binary 1,314,456 bytes (v0.1.1) | `gh api repos/tiktool/mouse-keys-record-replay/releases` |
| macOS build is universal (Apple silicon + Intel), macOS 11 or later | .github/workflows/release.yml:61-72; README downloads table |
| Windows 10 and 11 x64; Linux x64, X11 only (XInput 2 + XTEST) | README; src/input/linux.rs:50-69 |
| Single Rust binary, no runtime | Cargo.toml (tao, tray-icon, global-hotkey, native OS crates) |
| Starts at login, tray icon, status card on every monitor that never takes focus | README Features; src/autostart.rs; src/ui/ |
| No account, recordings stay local | README Privacy; no network code outside update.rs |
| Not code-signed on Windows, not notarized on macOS | README Install |
| No editor, scheduler, image or color search | README Limits |
| `[v0.1.3]` Playback speed: slider below the record and play buttons, 1/4x, 1/2x, 1x (default), 2x, 4x, Custom (number field, default 100x); shortcuts Ctrl/Cmd+Alt+Shift+F5 to F10 in that order; changes apply live during playback | unverified: per the coordinator's v0.1.3 spec, not yet in code |

Banned words (owner rule): superlatives and exclusivity claims, plus "undetectable", "anti-cheat", "bot", "game", "farm". Positioning is repetitive desktop tasks, testing and demos.

## 2. AlternativeTo

Submit at https://alternativeto.net/software/add/ (account needed). One app entry, then "Suggest as alternative" on each competitor page.

**Name:** Mouse Keys Record Replay

**Website:** https://mouserec.com

**Short description (one line):**
Free, open source tray app that records your mouse and keyboard and replays it once or on a loop with one shortcut, on Windows, macOS and Linux.

**Description:**
Mouse Keys Record Replay records what your mouse and keyboard do and plays it back with the original timing. Press Ctrl+Alt+Shift+F1 (Cmd+Option+Shift+F1 on macOS) to record, F2 to play once, F3 to loop. Touch the mouse or press any key and playback stops right away.

- Records moves, clicks, scrolls, drags and key presses together
- Optional random timing: each press shifts 0.1 to 0.5 seconds early or late per run, positions stay exactly as recorded
- [v0.1.3] Playback speed from 1/4x to 4x, or a custom multiplier, changeable while it plays (Ctrl/Cmd+Alt+Shift+F5 to F10)
- Lives in the tray, starts at login, status card on every monitor
- Updates itself from GitHub releases and checks an Ed25519 signature before installing
- Single native Rust binary, about 0.8 MB on Windows and 1.1 to 1.3 MB on macOS and Linux
- No account, recordings stay on your computer
- MIT licensed

Limits: Linux needs an X11 session (no Wayland). There is no editor, scheduler or image search; it replays exactly what you recorded. (see gate 2a)

**License:** Free, Open Source (MIT)

**Platforms:** Windows, Mac, Linux

**Categories:** Productivity > Automation (or "Macro Recorder" if offered as a category/feature)

**Tags / features to tick:** macro-recorder, mouse-recorder, keyboard-recorder, record-and-replay, automation, auto-clicker, tray-app, portable, lightweight, open-source, no-registration-required, hotkeys, cross-platform, rust

**Suggest as an alternative to** (pages from the research; each a one-line "why" in the suggestion box):

| Page | Suggestion note |
| --- | --- |
| TinyTask | Same record, play, loop idea, also on macOS and Linux (X11), MIT source |
| Pulover's Macro Creator | For people who want recording without scripting; Windows, macOS, Linux |
| Jitbit Macro Recorder | Free and MIT for plain record and replay without editor features |
| AutoHotkey (recorder use case) | Records instead of writing a script; runs on Windows, macOS, Linux |
| CrossMacro | Small native binary with three fixed shortcuts; Linux support is X11 |
| PyMacroRecord | Adds a macOS build; native binary, no Python needed |
| atbswp | Adds macOS; native binary |
| Macro Recorder (macrorecorder.com) | Free MIT recorder for simple record and replay |
| Keyboard Maestro (recording use case) | Free recorder for the record-and-replay part of the job |
| Keysmith | No macro count limit, MIT |
| Mini Mouse Macro | Open source, signed updates, three platforms |
| GS Auto Clicker / OP Auto Clicker | Records full click and key sequences with their timing |
| Actiona | Adds macOS, no scripting needed |
| Repeat | No Java needed |
| ReMouse | Free and MIT |
| BiggerTask, LinuxTask, tinymac | Same idea on all three desktops |


## 3. Product Hunt

**Name:** Mouse Keys Record Replay

**Tagline (52 chars):** Record your mouse and keys, replay them in one press

**Topics:** Productivity, Open Source, Mac, Windows, Linux, Developer Tools

**Description (260 chars max field):**
A small tray app that records your mouse and keyboard and plays it back once or on a loop. Three shortcuts, no editor, no account. (see gate 2a) Any real input stops playback. Random timing and [v0.1.3] playback speed built in. Windows, macOS, Linux (X11). MIT.

(Without v0.1.3: drop "and playback speed". Length checked under 260 with the bracket text removed.)

**First comment (maker):**

Hi Product Hunt, I am George from B4Battle LLC.

I kept doing the same few clicks and key presses over and over: filling the same test form, resetting a demo, stepping through a setup screen. TinyTask is the tool people point to for that, but it has no macOS or Linux build, and on macOS and Linux most options are either scripting tools or paid apps with full editors.

So we built Mouse Keys Record Replay:

- Ctrl+Alt+Shift+F1 (Cmd+Option+Shift+F1 on a Mac) to record, F2 to play once, F3 to loop.
- Move the mouse or press any key and playback stops. Playback marks its own events, so it can tell your hand from its replay.
- An optional random timing toggle shifts each press 0.1 to 0.5 s per run. Positions never move.
- [v0.1.3] Playback speed: 1/4x, 1/2x, 1x, 2x, 4x or a custom multiplier, from a slider or F5 to F10, and it changes live while the recording plays.
- It updates from GitHub releases and checks an Ed25519 signature before it installs anything.
- The Windows exe is under 1 MB. It is one Rust binary with no runtime.

What it does not do, so nobody is surprised: no Wayland yet (Linux is X11), no editor, no scheduler, no image search. (see gate 2a) The Windows build is not code-signed and the Mac app is not notarized yet, so the first launch shows a system warning.

It is MIT licensed, the source is on GitHub, and there is no account. I would love to hear what repetitive task you would use it for, and what is missing for you.

**Gallery captions (4):**
1. "Three shortcuts: record, play once, loop" (image: shortcut table over the tray menu)
2. "A status card on every monitor that never steals focus" (image: recording card)
3. "Touch the mouse and playback stops" (image: loop card, then stopped state)
4. "[v0.1.3] Slow it down or speed it up while it plays" (image: speed slider). Pre v0.1.3 replacement: "Windows, macOS and Linux, one small native binary"

## 4. SourceForge

**Project name:** Mouse Keys Record Replay
**Unix name:** mouse-keys-record-replay
**Summary (70 chars max):** Record mouse and keyboard, replay once or loop. Win, macOS, Linux. MIT
**Categories:** Desktop Automation, Testing, Accessibility (avoid "Games")
**License:** MIT
**OS:** Windows, macOS, Linux
**Programming language:** Rust
**Description:**
Mouse Keys Record Replay is a tray app that records mouse moves, clicks, scrolls, drags and key presses with their timing and plays them back once or on a loop. Ctrl+Alt+Shift+F1 records, F2 plays once, F3 loops (Cmd+Option+Shift on macOS). Any real input stops playback. Optional random timing, [v0.1.3] adjustable playback speed, Ed25519-signed auto-update, no account. Linux support needs X11. Source and releases: https://github.com/tiktool/mouse-keys-record-replay

### Mirroring GitHub releases

SourceForge has a built-in GitHub import that mirrors releases into the Files tab:

1. Create the project at https://sourceforge.net/create/ (B4Battle LLC account).
2. Admin > Tools > Import > "Import from GitHub" (the "GitHub Importer"), or use the "Sync releases from GitHub" option on the Files tab, pointing at `tiktool/mouse-keys-record-replay`. unverified: the exact menu names; SourceForge changed this UI more than once. Check the current docs at https://sourceforge.net/p/forge/documentation/GitHub%20Importer/ before starting.
3. Set it to keep syncing so each new tag's assets (`*-windows-x64.exe`, `*-macos.zip`, `*-linux-x64`, the `.sig` files and `SHA256SUMS`) appear automatically.
4. Fallback if the sync is unavailable: a release job step that uploads assets with `rsync -e ssh <files> <user>@frs.sourceforge.net:/home/frs/project/mouse-keys-record-replay/<tag>/` using a dedicated SourceForge SSH key stored as a GitHub Actions secret. Effort: about 1 hour plus one test release.
5. Set the default download per OS on the Files tab (the "i" button on each file) so the green button matches the visitor's OS.
6. Point "Project website" to https://mouserec.com and the "Code" link to GitHub so SourceForge is a mirror, not a fork.

## 5. Softpedia, MajorGeeks, Uptodown

Softpedia (https://www.softpedia.com/user/submit.shtml), one submission per OS (Windows, Mac, Linux sections are separate). MajorGeeks (contact form / "Submit Software" link, lists Windows software). Uptodown (developer console at https://developers.uptodown.com, Windows and Mac).

**Common submission text:**

Title: Mouse Keys Record Replay 0.1.3 [v0.1.3] (or 0.1.1 before release)
Publisher: B4Battle LLC
License: Freeware, Open Source (MIT)
Homepage: https://mouserec.com
Download URL (direct, versioned): https://github.com/tiktool/mouse-keys-record-replay/releases/download/v<version>/<asset>
Size: about 0.8 MB (Windows), 1.1 MB (macOS zip), 1.3 MB (Linux)
Requirements: Windows 10/11 x64; macOS 11+ (Apple silicon or Intel); Linux x64 with an X11 session

Short description (about 150 chars):
Record your mouse and keyboard and replay it once or on a loop with one shortcut. Stops when you move. Free and open source.

Long description:
Mouse Keys Record Replay is a small tray app for repetitive desktop tasks. Press Ctrl+Alt+Shift+F1 and do the task once; press it again to stop. Ctrl+Alt+Shift+F2 plays it back once and Ctrl+Alt+Shift+F3 repeats it until you stop it. On macOS the shortcuts use Cmd+Option+Shift.

Mouse moves, clicks, scrolls, drags and key presses are recorded together with their timing. Playback stops the moment you touch the mouse or keyboard, and any keys or buttons still held are released. An optional random timing toggle shifts each press by 0.1 to 0.5 seconds on every run while keeping positions exact. [v0.1.3] Playback speed can be set to 1/4x, 1/2x, 1x, 2x, 4x or a custom value, and can be changed while a recording is playing.

The app lives in the tray, starts at login and shows a status card that never takes focus. It updates from its GitHub releases and checks an Ed25519 signature before installing. There is no account and recordings stay on your computer.

Limits: Linux needs an X11 session. There is no editor, scheduler or image search (see gate 2a). The Windows build is not code-signed and the macOS app is not notarized, so the first launch may show a system warning.

Changelog line for each version: copy the GitHub release notes, plain language.

Note for these sites: they scan with many antivirus engines. An unsigned exe with a global keyboard hook can draw heuristic flags. Before submitting, upload the exe to VirusTotal, save the report link, and if any engine flags it, file a false-positive report with that vendor first (see long-game.md, risks).

## 6. MacUpdate

Submit via https://www.macupdate.com/developers (developer account).

Title: Mouse Keys Record Replay
Short description (under 80 chars): Record mouse and keyboard, replay once or on a loop with one shortcut
Category: Productivity / Utilities
Price: Free
Requirements: macOS 11.0 or later, Apple silicon or Intel
Description:
A menu bar app that records what your mouse and keyboard do and plays it back. Cmd+Option+Shift+F1 starts and stops recording, Cmd+Option+Shift+F2 plays it once and Cmd+Option+Shift+F3 loops it. Touch the mouse or press a key and playback stops. Optional random timing per run, [v0.1.3] playback speed from 1/4x to 4x or custom, changeable live. Universal binary, about 1.1 MB zipped. MIT licensed, no account, updates checked with an Ed25519 signature.

It needs Accessibility and Input Monitoring permission in System Settings to record and replay input. The app is not notarized yet, so the first launch needs right-click > Open (or System Settings > Privacy and Security > Open Anyway).

## 7. Flathub

Flathub requires a manifest PR to https://github.com/flathub/flathub and AppStream metadata. Draft of the AppStream summary and description:

App ID: com.mouserec.MouseKeysRecordReplay (reverse DNS of the owned domain mouserec.com)
Name: Mouse Keys Record Replay
Summary (Flathub asks for under about 35 chars, no trailing period): Record and replay mouse and keyboard
Developer: B4Battle LLC
License: MIT (project_license), CC0-1.0 (metadata_license)
Description:
Records mouse moves, clicks, scrolls and key presses with their timing and plays them back once or on a loop from three keyboard shortcuts. Any real input stops playback. Optional random timing per run. Requires an X11 session; Wayland is not supported.
Categories: Utility
Keywords: macro, recorder, automation, mouse, keyboard, replay

Packaging notes: the app needs `--socket=x11`, `--share=network` (update check) and the tray. The in-app self-updater must be disabled in the Flatpak build, because Flathub updates the app itself; that needs a build flag. Wayland-only desktops will not work, so the description must keep the X11 line. Effort: 1 to 2 days including review.

## 8. GitHub repo topics

Set with: `gh repo edit tiktool/mouse-keys-record-replay --add-topic <topic>` (max 20 topics).

macro-recorder, mouse-recorder, keyboard-recorder, record-and-replay, automation, desktop-automation, auto-clicker, tinytask-alternative, macro, input-recorder, tray-app, rust, cross-platform, windows, macos, linux, x11, productivity, testing, open-source

About field: "Record mouse and keyboard, replay once or on a loop with one shortcut. Windows, macOS, Linux (X11). MIT."
Website field: https://mouserec.com
