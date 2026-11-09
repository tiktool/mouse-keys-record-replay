# Community posts: drafts

DRAFTS ONLY. Nothing has been posted. Lines that list "no editor" as a limit get a `[v0.1.4]` recording-history line added once v0.1.4 (F4 panel, list of past recordings) is live; see listings.md gate 2a. Hold every post until mouserec.com passes live checks. Lines marked `[v0.1.3]` need the v0.1.3 release live first (see listings.md section 0); delete them to post earlier.

Account rule for every venue: post from the maker's own long-standing account, disclose "I made this" in the first line, stay in the thread for the first several hours, and answer every question. Never ask anyone to upvote.

## 1. Show HN

Rules (from https://news.ycombinator.com/showhn.html, fetched 2026-11-09):
> "Show HN is for something you've made that other people can play with."
> "The project must be something you've worked on personally and which you're around to discuss."
> Don't ask for upvotes or comments from friends ("not ok"); no landing pages or fundraisers; do not submit work that is not ready for users to try.

Submit the GitHub repo URL (a runnable project), not the marketing site.

**Title:**
Show HN: Mouse Keys Record Replay - record mouse and keyboard, replay with a hotkey

(Uses a hyphen, not a dash. Under 80 chars.)

**URL:** https://github.com/tiktool/mouse-keys-record-replay

**Text (first comment, posted right after submitting):**

I made a small tray app that records mouse and keyboard input and replays it, once or in a loop. Ctrl+Alt+Shift+F1 starts and stops recording, F2 plays once, F3 loops (Cmd+Option+Shift on macOS). It is MIT, written in Rust, and runs on Windows, macOS and Linux under X11.

The part that took the most care is telling the user apart from the replay. Playback stops on any real mouse or keyboard input, so the listener has to ignore our own synthetic events. Each OS gives a different way to do that:

- Windows: low-level hooks (WH_KEYBOARD_LL / WH_MOUSE_LL), and every SendInput event carries a fixed dwExtraInfo value that the hook checks.
- macOS: a listen-only CGEventTap, and injected CGEvents carry a marker in EVENT_SOURCE_USER_DATA.
- Linux: XInput 2 raw events for listening and XTEST for injection; the listener matches the source device id against the XTEST devices.

Other details: timestamps are microseconds from the start of the recording, playback sleeps in at most 2 ms slices toward each due time (with timeBeginPeriod(1) on Windows), and anything still held when playback stops is released. An optional random timing mode shifts each press by 0.1 to 0.5 s while keeping positions, hold lengths and order. [v0.1.3] Playback speed (1/4x to 4x, or a custom multiplier) can be changed while a loop runs.

Updates come from GitHub releases and are installed only after an Ed25519 signature check. The Windows exe is about 760 KB.

Limits: no Wayland, no editor, no scheduler, no image matching. Windows build is not code-signed and the macOS app is not notarized yet.

I would like feedback on the input layer, especially from anyone who has dealt with Wayland input capture, which is the obvious next step.

(Evidence: src/update.rs:156-159 discards the download when verify fails.)

## 2. Reddit

Fetch method note: reddit.com blocked direct fetches from this machine on 2026-11-09. Rules below were read from a Redlib mirror of each sidebar (redlib.scsimedia.duckdns.org, 2026-11-09) plus news coverage where stated. Re-read each sidebar on reddit.com right before posting; mods change rules.

### 2a. r/software

Rules quoted (sidebar via mirror):
> Rule 2: "No Spam/promotional posts pushing commercial software"
> Rule 3: "All submissions must be in English"
The sub shows a "Self-Promotion Wednesdays" post flair; the sidebar does not describe it. Plan: post on a Wednesday with that flair, since the app is free and MIT (not commercial).

**Title:** I made a free, open source mouse and keyboard recorder for Windows, macOS and Linux

**Body:**
Disclosure: I am the developer (B4Battle LLC). It is free and MIT licensed, no account, no paid tier.

Mouse Keys Record Replay records what your mouse and keyboard do and plays it back. Three shortcuts: Ctrl+Alt+Shift+F1 to record, F2 to play once, F3 to loop (Cmd+Option+Shift on a Mac). Touch the mouse or press a key and playback stops.

Useful for repetitive desktop chores, refilling the same test form, resetting a demo, or recording a sequence for a screen capture.

- Random timing toggle (each press shifts 0.1 to 0.5 s, positions exact)
- [v0.1.3] Playback speed 1/4x to 4x or custom, changeable while playing
- Tray app, starts at login, under 1.5 MB on every OS
- Signed auto-update from GitHub releases

Limits: Linux needs X11, no editor or scheduler. Windows build is unsigned, so SmartScreen will warn once.

Source and downloads: https://github.com/tiktool/mouse-keys-record-replay

Happy to answer questions or take feature requests.

### 2b. r/macapps

Rules quoted (sidebar via mirror, 2026-11-09):
> Rule 3: "One app post per developer per 30 days, counted from your last app post, even if removed. Updates count."
> "The App Pile: one entry per dev per month. Apps posted in the main feed can't enter."
> Comments must "ALWAYS disclose it's your app."
> Rule 6: "Main feed posts need 200+ Reddit karma, a 60+ day old account, a pricing flair and 4 labels:" Problem, Comparison, Pricing, Trust.
> "Trust: MAS | Dev Flair | GitHub (100+ stars or LinkedIn) | Website (notarized, LinkedIn, privacy policy)"
> Rule 1: "Free flair: not if any paid option exists." Open source projects should prefix the title with [OS].
> A second self-promo removal within 90 days triggers a 60-day promo break.

Eligibility today: the app is not notarized and not on the Mac App Store, and the repo is far below 100 stars. Trust can still come from "GitHub (... or LinkedIn)" if the post links the maker's LinkedIn, but that reading is unverified. Safe route now: the monthly App Pile thread. Main feed after notarization (website trust path) or 100+ stars. Use the Free flair only while no paid option exists (if a Pro add-on ships, the flair changes).

**App Pile entry / main feed post title:**
[OS] Mouse Keys Record Replay: record your mouse and keyboard, replay with one shortcut

**Body (with the four labels the rule asks for):**
I made this (disclosure: developer, B4Battle LLC).

**Problem:** Repeating the same clicks and keystrokes on a Mac, for test forms, demo resets or setup steps. TinyTask has no Mac version, and the Mac options I found were scripting tools or paid macro editors.

**Comparison:** It does one thing: record and replay, with three shortcuts (Cmd+Option+Shift+F1 record, F2 play once, F3 loop). No editor, conditions or scheduler like Keyboard Maestro has; if you need those, this is not a replacement. Any real input stops playback. Optional random timing per run. [v0.1.3] Playback speed 1/4x to 4x or custom, live.

**Pricing:** Free, MIT, no account, no in-app purchases.

**Trust:** Source on GitHub (https://github.com/tiktool/mouse-keys-record-replay), universal binary built in public CI, updates verified with an Ed25519 signature. Not notarized yet, so the first launch needs right-click > Open. It needs Accessibility and Input Monitoring permission to record and replay.

### 2c. r/linux

Rules quoted (sidebar via mirror, 2026-11-09):
> "Submitting your own original content is welcome on r/linux, but we do ask that you contribute more than just your own content"
> "as well as require you to interact with the comments of your submission."
> "We set that no more than 10% of your posts should be your content."
> Relevance rule: "Posts should follow what the community likes: GNU, Linux kernel, developers of open source software"
A May 2026 thread proposed banning personal projects under 3 months old; whether it was adopted is unverified. The repo history starts at v0.1.0 (commit d58329a); check its age against that rule at post time.

To stay under the 10% rule, the posting account needs at least 9 non-promotional posts or comments in r/linux for each self post. Fit: post the technical article (section 3) as a link, not the download page.

**Title:** How a small Rust app records and replays input on X11 with XInput 2 raw events and XTEST

**Body (text post variant):**
I wrote a MIT-licensed mouse and keyboard recorder (disclosure: mine). The Linux backend listens for XI_RawKeyPress / RawButtonPress / RawMotion on the root window through XInput 2 and replays through XTEST. To stop playback when the user touches anything, it compares each raw event's source device id with the XTEST device ids, so our own replay never counts as user input.

It needs X11. I have not shipped Wayland because there is no portable way to read global input and inject it without compositor-specific protocols or uinput with elevated permissions. I would like to hear how others handle that, and whether the libei / RemoteDesktop portal route is worth it for a tool like this.

Write-up: <dev.to link>
Code: https://github.com/tiktool/mouse-keys-record-replay (src/input/linux.rs)

### 2d. r/opensource

Rules quoted (sidebar via mirror, 2026-11-09):
> "No Spam / Excessive self-promotion -"
> "Reddit recommends that <10% of your posts promote your content."
> "It's perfectly fine to be a redditor with a website, it's not okay to be a website with a reddit account."
> Flair: "`Promotional` is when you are sharing a project, yours or otherwise."

Newer policy (news coverage, runtimewire.com, 2026-10-25, not read from the live sidebar): promoted projects must be "actively maintained for at least a year", have multiple contributors ("subjectively, three or more"), and the sub is "excluding projects that use AI, are made by AI, or are intended for AI".

Verdict: HOLD. The project is weeks old and has one contributor identity, so it does not meet the reported one-year and three-contributor bar. Do not post here until both are true and the live sidebar is re-read; development of this project used AI coding tools, so the AI clause would also need an honest answer to the mods. Draft kept for that day:

**Title:** Mouse Keys Record Replay: MIT mouse and keyboard recorder for Windows, macOS and Linux (X11)

**Body:**
Disclosure: I maintain this project. It records mouse and keyboard input and replays it with three shortcuts, stops on any real input, and updates itself through Ed25519-signed GitHub releases. MIT, Rust, no account. Contributions welcome, especially a Wayland backend: https://github.com/tiktool/mouse-keys-record-replay

### 2e. r/automation

Rules: the sidebar seen through the mirror shows only the description ("Thought-provoking discussions on automation, ChatGPT, and AI." / "Automatizing your life."), and the wiki index reads "Placeholder". No self-promotion rule could be read (unverified). Apply the Reddit-wide guideline instead: keep self-promotion under 10% of activity, disclose, and lead with the use case. Re-check the sidebar on reddit.com before posting.

**Title:** For repetitive desktop clicks that do not justify a script, I built a record-and-replay tray app

**Body:**
Disclosure: I made this. Free and MIT.

Not every repetitive task is worth an automation platform. Some are "click here, type this, tab, enter" twenty times. For those I wanted something closer to a tape recorder than a workflow builder:

1. Ctrl+Alt+Shift+F1, do the task once, press it again.
2. Ctrl+Alt+Shift+F2 to replay once, F3 to loop.
3. Move the mouse to stop.

Random timing per run is optional. [v0.1.3] Playback speed can go down to 1/4x to watch a step, or up to 4x or a custom multiplier when the target app keeps up.

Honest limits: it replays screen positions, so windows must be where they were. No conditions, no image matching, no scheduler. Linux needs X11.

Windows, macOS, Linux: https://github.com/tiktool/mouse-keys-record-replay

What do you use for these small, ugly, repetitive jobs?

## 3. dev.to / Hashnode article (about 900 words)

Tags: rust, opensource, automation, crossplatform
Canonical URL: set to the mouserec.com blog copy if one is published, otherwise leave dev.to as canonical.

---

# Recording and replaying mouse and keyboard input on Windows, macOS and Linux in Rust

Mouse Keys Record Replay is a small MIT tray app: press a shortcut, do a task, press it again, then replay it once or on a loop. This post walks through how the engine works on three operating systems, and where each OS made it harder than expected.

## One event type, three native encodings

A recording is a list of steps. Each step is a timestamp in microseconds since recording started, plus one event:

```rust
pub enum Ev {
    Key { code: u32, scan: u32, flags: u64, down: bool },
    Move { x: i32, y: i32 },
    Button { b: u8, down: bool, x: i32, y: i32, n: u8 },
    Wheel { dx: i32, dy: i32 },
}
```

Key codes stay platform native: a Windows virtual key plus scan code and hook flags, a macOS keycode plus CGEventFlags, or an X keycode. That was a deliberate trade. A recording is replayed on the system family that made it, so there is no key mapping table to maintain and nothing is lost in translation. The cost is that a recording made on Windows will not play on a Mac.

Every platform backend exposes the same small surface: start a global listener, inject one event, read the cursor position, check whether any key is physically down, and check input permissions.

## Listening

- **Windows** installs low-level hooks with `SetWindowsHookExW(WH_KEYBOARD_LL)` and `WH_MOUSE_LL` on a dedicated thread with its own message loop. Dropping the hook object posts `WM_QUIT` to that thread and joins it.
- **macOS** creates a listen-only `CGEventTap` at the session level. It needs two permissions: Accessibility (checked with `AXIsProcessTrusted`) and Input Monitoring (`CGPreflightListenEventAccess`). If they are missing, the app shows a message naming both and an Open System Settings button.
- **Linux (X11)** asks for XInput 2.2 and selects raw key, button and motion events on the root window for all devices. Raw events arrive whether or not the app has focus. Wayland deliberately does not offer this to normal clients, which is why the app does not support Wayland today.

## Telling the user apart from the replay

The safety rule is simple: any real input stops playback. The hard part is "real". While the app replays, the listener also sees every injected event. Without a way to tell them apart, playback would stop itself on its first move.

Each OS offers a different hook for this:

- **Windows:** every `SendInput` event carries a fixed `dwExtraInfo` value. The hook reads it back and marks matching events as injected.
- **macOS:** injected `CGEvent`s set `EVENT_SOURCE_USER_DATA` to the same marker before being posted at the HID level. The tap reads that field.
- **Linux:** at startup the backend lists XInput devices and remembers the ids of the XTEST devices. A raw event whose source is one of those came from our own `xtest_fake_input` call.

The engine side is one function. Injected events are ignored; real events are appended to the recording while recording, and flip an abort flag while playing.

## Playback timing

Playback walks the steps against a single start instant. For each step it computes the due time and sleeps toward it in slices of at most 2 ms, checking the abort flag every slice. That keeps the reaction to a user interrupt within a couple of milliseconds without spinning a core. On Windows the app calls `timeBeginPeriod(1)` for the duration of playback, since the default timer resolution would otherwise make short sleeps much longer than requested.

When playback ends, normally or by interrupt, it releases every key and mouse button it still holds. A recording that stops in the middle of a drag must not leave the left button down.

Loops have a floor of 50 ms per pass, so a near-empty recording cannot spin the processor.

## Random timing without breaking input

The optional random mode exists because some people want a loop that does not repeat with machine precision. The naive version, adding noise to every timestamp, breaks things: a key release can land before its press, a double click turns into two single clicks, and a held key's auto-repeat drifts away from it.

The implementation moves whole blocks instead. A block starts at each fresh press (a key that is not already held, or a mouse button with click count below 2) and carries everything up to the next press. Each block gets one offset between 0.1 and 0.5 seconds, earlier or later. Key repeats, the second click of a double click and scroll ticks ride with the press before them, and a block is never pulled in front of the previous one. Positions, keys, hold lengths and order stay exactly as recorded. Unit tests run the transform hundreds of times and assert order, hold lengths and offset ranges.

[v0.1.3] Playback speed works on the same timeline: a slider (1/4x, 1/2x, 1x, 2x, 4x or a custom multiplier, default 100x) or Ctrl/Cmd+Alt+Shift+F5 to F10, and a change applies to the pass that is already playing. (Before publishing, replace this paragraph with how the shipped v0.1.3 code scales due times, read from the merged source.)

## Updating safely

The app checks the latest GitHub release every 12 hours. It downloads the asset for its platform plus a detached signature, verifies the signature with Ed25519 (`ed25519-dalek`, `verify_strict`) against a public key compiled into the binary, and refuses to install on a mismatch. The install scripts check the same signature before installing.

## What it costs

One Rust binary built with `opt-level = "z"`, LTO and stripped symbols. The v0.1.1 release is 777,728 bytes on Windows, about 1.1 MB zipped on macOS (universal, Apple silicon and Intel) and 1.3 MB on Linux.

## Limits

No Wayland, no editor, no scheduler, no image matching. Playback replays screen positions, so windows need to be where they were during recording. The Windows build is not code-signed and the macOS app is not notarized yet.

Code: https://github.com/tiktool/mouse-keys-record-replay. Issues and pull requests welcome, especially for Wayland.
