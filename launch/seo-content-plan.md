# SEO content plan: mouserec.com

Status 2026-11-09: `/` returns 200 (holding page); every sub-page below returned 404 (curl, 2026-11-09). The Nuxt site (D:\Works\mouserec-site, commit 2a8b638) has these pages built but not deployed: /, /download, /features, /how-it-works, /faq, /changelog, /compare, /windows, /macos, /linux, /privacy, /terms. Deploy first; none of this plan works against 404s.

Locales: en has no prefix; the 16 prefixes are /ar/ /de/ /el/ /es/ /fr/ /id/ /it/ /ja/ /ko/ /pt-br/ /ro/ /ru/ /tr/ /vi/ /zh-hans/ /zh-hant/. Every new page ships in en first, then in the locales in the order in section 3. Each page keeps hreflang, JSON-LD and a sitemap entry, as the site already does for its current pages.

Copy rules for every page: claims checked against the code (listings.md section 1), no "best", "only", "the first", no "choose X if" lines about other products, no game or anti-cheat angle. The task is repetitive desktop work, testing and demos. Playback speed topics (marked `[v0.1.3]`) wait for v0.1.3 to ship.

New pages live under /guides/<slug> (new route, one template). Priority: P1 = write in the first 30 days, P2 = by day 90, P3 = later.

## 1. Topics

Format: target query | intent | outline | internal links.

### Cluster A: TinyTask on other systems (biggest research gap)

1. **P1 /macos (expand existing page)**
   Query: tinytask for mac. Intent: wants the TinyTask workflow on macOS.
   Outline: what TinyTask does and that it has no macOS build; install line and zip; the three Cmd+Option+Shift shortcuts; Accessibility and Input Monitoring permissions with screenshots; first launch of a non-notarized app; limits.
   Links: /download, /how-it-works, /faq, /compare.

2. **P1 /linux (expand existing page)**
   Query: tinytask for linux. Intent: same, on Linux.
   Outline: X11 requirement and how to check the session type (`echo $XDG_SESSION_TYPE`); install line; tray needs AppIndicator; shortcuts; limits (no Wayland).
   Links: /download, /how-it-works, /faq.

3. **P1 /guides/tinytask-alternative**
   Query: tinytask alternative. Intent: comparing tools.
   Outline: what people use TinyTask for; the same workflow here (record, play, loop); what differs (3 OSes, MIT, signed updates, random timing, [v0.1.3] speed); what it does not do (no compile to exe); facts table, no verdict lines.
   Links: /compare, /features, /download.

4. **P1 /guides/is-tinytask-safe**
   Query: is tinytask safe. Intent: worried about antivirus warnings or downloads.
   Outline: why tiny recorders trigger antivirus heuristics (hooks plus input injection); how to check any download (official source, hashes, VirusTotal); how this app's signatures and SHA256SUMS work; honest note that the exe is not code-signed yet.
   Links: /download, /faq, /privacy.

5. **P2 /guides/tinytask-wayland**
   Query: tinytask wayland / macro recorder wayland. Intent: Wayland user stuck.
   Outline: why Wayland blocks global capture and injection; logging into an X11 session; current status of Wayland support (not supported, contributions welcome). Honest page: it ranks for a real pain and must not promise anything.
   Links: /linux, /faq.

6. **P2 /guides/tinytask-windows-11**
   Query: tinytask windows 11 / tinytask not working windows 11. Intent: troubleshooting.
   Outline: SmartScreen first run; elevated windows (low-level hooks cannot drive admin windows from a normal process, unverified for this app, test before publishing); DPI scaling and positions.
   Links: /windows, /download, /faq.

### Cluster B: the core task

7. **P1 /guides/record-mouse-clicks-and-replay**
   Query: record mouse clicks and replay. Intent: wants to do it now.
   Outline: 3 steps with shortcuts per OS; what gets recorded (moves, clicks, drags, scrolls, keys with timing); stopping; loop; tips (keep windows in place).
   Links: /how-it-works, /download, /features.

8. **P1 /guides/auto-clicker-that-records-keyboard**
   Query: auto clicker that records keyboard. Intent: clickers only click; wants keys too.
   Outline: difference between an interval clicker and a recorder; recording mouse and keyboard together; random timing; [v0.1.3] speed.
   Links: /features, /compare, /download.

9. **P1 /guides/mouse-and-keyboard-recorder**
   Query: mouse and keyboard recorder (free, open source variants). Intent: tool search.
   Outline: what to look for (stop on input, held-key release, update security, license); how this app handles each; download.
   Links: /features, /how-it-works, /download.

10. **P2 /guides/repeat-mouse-movements**
    Query: how to repeat mouse movements / record mouse movement. Intent: how-to.
    Outline: move events and timing; loop mode; per-pass floor; stop by moving the mouse.
    Links: /how-it-works, /features.

11. **P2 /guides/loop-a-recording**
    Query: loop macro until stopped / repeat recorded actions forever. Intent: how-to.
    Outline: F3 loop; stop with any input; what happens to held keys; using random timing on loops.
    Links: /how-it-works, /faq.

12. **P2 /guides/record-keyboard-macro**
    Query: record keyboard macro windows / mac. Intent: keyboard-only task.
    Outline: recording keystrokes with timing; key repeat behaviour; held modifiers released on stop.
    Links: /windows, /macos, /linux, /how-it-works.

13. **P2 /guides/random-delay-macro**
    Query: macro random delay / randomize click timing. Intent: wants less mechanical replay for testing or demos.
    Outline: how random timing moves whole press blocks by 0.1 to 0.5 s; what stays fixed; when to use it (UI timing tests, demo realism). No evasion framing.
    Links: /features, /how-it-works.

### Cluster C: playback speed `[v0.1.3]`

14. **P1 after v0.1.3 /guides/tinytask-speed**
    Query: tinytask speed / tinytask playback speed. Intent: wants faster or slower replay.
    Outline: speed steps 1/4x, 1/2x, 1x, 2x, 4x, Custom (default 100x); Ctrl/Cmd+Alt+Shift+F5 to F10; live change during playback; caveat that target apps must keep up at high speed.
    Links: /features, /how-it-works, /download.

15. **P1 after v0.1.3 /guides/speed-up-macro-playback**
    Query: speed up macro playback / fast forward macro. Intent: how-to.
    Outline: when speeding up works (form fills, menu paths) and when it breaks (apps that need load time); custom multiplier; going back to 1x with one shortcut.
    Links: /features, /faq.

16. **P2 after v0.1.3 /guides/slow-down-recorded-mouse-movements**
    Query: slow down recorded mouse movements / slow motion macro. Intent: watch or debug a recording, or make a demo readable.
    Outline: 1/4x and 1/2x for demos and checking steps; live change; pairs with screen recording.
    Links: /features, /how-it-works.

17. **P2 after v0.1.3 /guides/macro-playback-speed-shortcuts**
    Query: change macro speed hotkey. Intent: reference.
    Outline: table of F5 to F10 per OS; slider position; how the custom field works.
    Links: /features, /faq.

### Cluster D: use cases (testing, demos, chores)

18. **P2 /guides/automate-repetitive-desktop-tasks**
    Query: automate repetitive tasks on pc without coding. Intent: no-code chores.
    Outline: examples (same form, same export path, same setup steps); limits (positions, no conditions); when a script is the better tool, stated neutrally.
    Links: /, /features, /download.

19. **P2 /guides/record-ui-test-steps**
    Query: record and replay ui test / manual test replay tool. Intent: QA.
    Outline: replaying a smoke path on a build; random timing to shake out timing bugs; [v0.1.3] speed for fast reruns; limits (no assertions).
    Links: /features, /how-it-works.

20. **P3 /guides/demo-recording-reset**
    Query: repeat demo steps automatically / kiosk demo loop. Intent: demos and trade-show loops.
    Outline: loop a demo path; stop on touch; status card; [v0.1.3] slow speed for narrated demos.
    Links: /features, /download.

### Cluster E: trust and setup questions

21. **P1 /faq (extend)**
    Queries: mouse recorder macos permissions / input monitoring permission. Intent: setup blocker.
    Outline: why Accessibility and Input Monitoring are needed; where to grant them; how to reset them.
    Links: /macos, /how-it-works.

22. **P2 /guides/macro-recorder-privacy**
    Query: does macro recorder send data / offline mouse recorder. Intent: privacy check.
    Outline: no account; recordings stay local; the update check is the one network request; turning updates off.
    Links: /privacy, /faq.

23. **P2 /guides/verify-download-signature**
    Query: verify ed25519 signature download. Intent: technical trust.
    Outline: SHA256SUMS and .sig files; what the installers check; how to check by hand.
    Links: /download, /how-it-works.

24. **P3 /guides/open-source-macro-recorder**
    Query: open source macro recorder / mit mouse recorder. Intent: license-aware users.
    Outline: MIT terms in plain words; source link; build from source steps.
    Links: /, /compare, /download.

### Cluster F: named comparisons (facts tables, no verdicts)

25. **P2 /compare/pulovers-macro-creator**
    Query: pulover's macro creator alternative mac / linux. Intent: comparing.
    Outline: facts table (OS, license, editor, scheduling, size); where Pulover's does more (editor, AutoHotkey scripts); where this app differs (macOS and Linux, signed updates). No recommendation line.
    Links: /compare, /features.

26. **P2 /compare/jitbit-macro-recorder**
    Query: jitbit macro recorder free alternative. Intent: comparing on price.
    Outline: same facts-table format; note Jitbit has an editor and is paid.
    Links: /compare, /features.

27. **P3 /compare/crossmacro**
    Query: crossmacro alternative. Intent: Linux users comparing.
    Outline: facts table (CrossMacro supports Wayland, GPL-3.0, .NET); this app (needs X11, MIT, native). Honest page.
    Links: /compare, /linux.

28. **P3 /compare/autohotkey-recorder**
    Query: autohotkey macro recorder. Intent: wants recording without writing script.
    Outline: AutoHotkey is a scripting language on Windows; recorder add-ons; this app records with no script and runs on 3 OSes.
    Links: /compare, /windows.

## 2. Internal linking rules

- Every guide links to /download once in the first screen and to one feature page (/features or /how-it-works).
- Every OS-specific guide links to its OS page (/windows, /macos, /linux).
- /compare links out to each /compare/<name> page; each one links back.
- Home links to the three OS pages and to the P1 guides in a "Guides" block.
- Localized pages link to same-locale targets only (/de/guides/... to /de/download).
- New releases: /changelog entry links to the guide for the new feature (v0.1.3 entry to /guides/tinytask-speed).

## 3. Localization order

Pick by where the research showed demand for TinyTask-style tools (unverified per-locale volume; check GSC after 30 days and reorder):
1. es, pt-br, de, fr (P1 guides 1-4, 7-8)
2. ru, tr, id, vi, ja, ko (same set)
3. it, ro, el, ar (RTL check), zh-hans, zh-hant
Translate with human review for the P1 set; machine-drafted pages must be read by a native speaker before indexing.

## 4. Measurement

- Google Search Console: clicks and impressions per page and query, weekly. Baseline is 0 (site not deployed).
- Bing Webmaster plus IndexNow (already wired in the site) for fast indexing.
- Download count per release from `gh api repos/tiktool/mouse-keys-record-replay/releases` (baseline in long-game.md).
