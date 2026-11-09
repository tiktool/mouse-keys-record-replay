# Long game: Mouse Keys Record Replay

Plan for the year after launch. Effort is in focused working days (d) for one person. KPIs use sources anyone can re-run.

## 0. Baseline (2026-11-09)

Source: `gh api repos/tiktool/mouse-keys-record-replay/releases` and `gh api repos/tiktool/mouse-keys-record-replay`.

| Metric | Value |
| --- | --- |
| v0.1.1 binary downloads | Linux 10, macOS 7, Windows 3 (20) |
| v0.1.0 binary downloads | Linux 5, macOS 3, Windows 4 (12) |
| Stars | 1 |
| Public repo | yes (`private: false`) |
| Topics set | 10 (auto-clicker, automation, keyboard-recorder, linux, macos, macro-recorder, mouse-recorder, rust, tinytask-alternative, windows) |
| GSC clicks | 0 (site sub-pages not deployed, `/` is a holding page) |

KPI query for installs (sum of binary assets, signatures excluded):

```sh
gh api repos/tiktool/mouse-keys-record-replay/releases --paginate \
  --jq '[.[].assets[] | select(.name | test("\\.(sig)$|SHA256SUMS") | not) | .download_count] | add'
```

Install-script runs also hit these release URLs, so this counts both paths. Auto-updates also download the asset, so the number counts update traffic too; track the latest release on its own to see the new-plus-updated mix.

## 1. Money (keep the core free and MIT)

The free MIT core stays complete for what it does today. Paid things are additive, never a crippled free version.

| Option | What it is | Effort | Notes and risks |
| --- | --- | --- | --- |
| Signed and notarized builds | Windows code signing (OV or EV cert, or Azure Trusted Signing) and Apple Developer ID notarization, used for every release, free to users | 3d plus cert cost | Mostly a trust and antivirus fix, not revenue. Developer ID needs the Apple Account Holder (memory note 11-09). Unblocks r/macapps main feed and fewer SmartScreen warnings. |
| Store builds | Microsoft Store (free or a small price), Mac App Store | 5d per store | Mac App Store sandbox likely blocks a global event tap plus injection (unverified, test before planning revenue on it). Store listing is also a trust signal. |
| Pro add-on (paid licence) | Step editor (trim, delete, change delays), scheduling, named recordings library, image or colour triggers | editor 10d, scheduler 5d, library 4d, image triggers 15d | Open question for the owner: Pro in a closed add-on vs in the MIT tree behind a licence key (a key in MIT code is trivial to remove). Image triggers are the most requested in competitor reviews and the most work. Price research needed (Jitbit and Mini Mouse Macro are paid tools). |
| Business licence | Same binary, invoice, support email, a licence for teams that need paperwork | 2d | Low effort, low volume. Needs terms page update. |
| GitHub Sponsors | Sponsor button plus a FUNDING.yml | 0.5d | Small money, real goodwill. Do this week 1. |

Rule for any paid feature: copy describes what it does, never "unlocks" anything that was taken out of the free build.

## 2. Funnels into other B4Battle products

Superseded by decision 5 below: no cross-promotion in this kit. The table is kept for a later owner decision only.

The audience is people who automate repetitive desktop work, testers and demo makers. Cross-promotion must fit that audience or it reads as spam.

| Product | Fit | How | Effort |
| --- | --- | --- | --- |
| tik.tools | Low to medium: creators and developers who automate live-stream tooling | One "From the same team" line on /about-style footer, not in the app | 0.5d |
| stagevisuals | Low: show and lighting operators who repeat setup steps | Footer link only | 0.5d |
| tiklead, barebase, barestyle, achkit | Unverified fit: product scope not checked in this run | Owner decides; default is a footer "More from B4Battle LLC" page | 0.5d |

Do not add promotion inside the app (tray menu, status card or update notes). The app's trust story is "no account, one network request"; ads in it would break that.

## 3. Backlinks and authority

| Action | Effort | KPI |
| --- | --- | --- |
| Directory listings from listings.md (AlternativeTo, SourceForge mirror, Softpedia, MajorGeeks, Uptodown, MacUpdate, Product Hunt) | 3d total | listings live; referral clicks in analytics |
| Package managers (section 5) give high-authority links (winget-pkgs, Homebrew, Flathub, AUR pages) | see section 5 | each listing live |
| Show HN and the dev.to article | 1d plus a day of replies | referral sessions, stars in the 72 h after |
| "Awesome" lists on GitHub (awesome-macos, awesome-linux-software, awesome-rust under applications) via PRs, following each list's rules | 1d | PRs merged |
| Answer existing questions where the app is a real fit (Super User, Ask Ubuntu, Stack Exchange), with disclosure, only where the asker's need matches | 0.5d per month | accepted or upvoted answers |
| Write-ups on hooks and injection per OS (the dev.to article split into 3 deeper posts) | 3d | links from Rust and OS communities |

No paid links, no link swaps, no comment spam. Each would risk the domain and the brand.

## 4. Localized content

The site ships 17 locales. Order and rules are in seo-content-plan.md section 3. Effort: about 0.5d per guide per locale with native review, so the P1 set (6 guides) across 4 first-wave locales is about 12d. Cheaper path: translate only the 3 OS pages and /download first (4 pages x 16 locales), then measure in GSC which locales get impressions and translate guides only there.

## 5. Package-manager presence

A separate stream (the "Package manager distribution" agent) owns this; this plan only sets the order and KPIs.

| Channel | Effort | Blocker |
| --- | --- | --- |
| winget | 1d | unsigned exe can pass but SmartScreen still warns; manifest review by maintainers |
| Scoop (own bucket first, then extras) | 0.5d | none known |
| Chocolatey | 1d | moderation queue; antivirus scan of the package |
| Homebrew cask (own tap first) | 0.5d | homebrew/cask main repo wants notarized apps and notability (unverified thresholds, check their docs) |
| Flathub | 3d | needs the self-updater off in the Flatpak build, X11 socket permission, review |
| AUR | 0.5d | none known |
| Snap | 2d | classic confinement or X11 interface review (unverified which) |

KPI: number of channels live; installs per channel where the channel publishes counts (Homebrew analytics, Flathub stats, Chocolatey downloads).

## 6. 30 / 90 / 365-day plan

### Day 0 to 30

| Item | Effort | KPI |
| --- | --- | --- |
| Deploy mouserec.com (sub-pages are 404 today) | 0.5d | all 12 pages return 200 |
| GSC and Bing Webmaster set up, sitemap submitted | 0.5d | sitemap read, pages indexed |
| VirusTotal check and false-positive reports for all 3 binaries | 1d | 0 detections on the Windows exe, or reports filed |
| Real screenshots and a 20 s demo video | 1d | assets in listings |
| GitHub Sponsors | 0.5d | button live |
| Directory listings (non-Reddit) | 3d | 5+ listings live |
| Show HN, dev.to article | 1.5d | posted, replies answered |
| Reddit: r/software, r/linux, r/automation, r/macapps App Pile, per community.md rules | 1d | posts not removed |
| v0.1.3 speed ships; listings flip to v0.1.3 copy | owned by the speed stream | release live and checked on 3 OSes |
| P1 guides (seo-content-plan.md) in en | 5d | pages indexed |
| Scoop, AUR, own Homebrew tap | 1.5d | 3 channels live |

Targets by day 30: 300 binary downloads total, 25 stars, 100 GSC clicks per week by week 4 (rough targets from a zero base; revise after week 2 with real numbers).

### Day 31 to 90

| Item | Effort | KPI |
| --- | --- | --- |
| Windows code signing and macOS notarization | 3d plus cert | first signed release |
| winget, Chocolatey, Flathub | 5d | 3 more channels live |
| P2 guides in en, P1 guides in first-wave locales | 10d plus review | pages indexed per locale |
| Awesome-list PRs | 1d | 2+ merged |
| r/macapps main feed (after notarization or 100 stars) | 0.5d | post approved |
| Decide Pro scope (editor first), pricing research | 2d | owner decision recorded |

Targets by day 90: 2,000 total binary downloads, 150 stars, 500 GSC clicks per week.

### Day 91 to 365

| Item | Effort | KPI |
| --- | --- | --- |
| Wayland support research and a prototype (portal or libei route) | 10d to 20d | prototype on GNOME and KDE |
| Pro editor and scheduler (if approved) | 15d | paying users, refund rate |
| Microsoft Store listing | 5d | listing live, store installs |
| P3 guides and comparison pages; second and third locale waves | 15d | GSC clicks per locale |
| Re-apply to r/opensource once eligible (1 year maintained, 3+ contributors) | 0.5d | post allowed |
| Quarterly release notes post (dev.to or own blog) | 0.5d each | referral sessions |

Targets by day 365: 15,000 total binary downloads, 1,000 stars, 3,000 GSC clicks per week, 7 package channels live.

## 7. Risks

| Risk | Effect | Mitigation | Effort |
| --- | --- | --- | --- |
| Antivirus false positives on the unsigned exe (hooks plus injection look like keyloggers to heuristics) | downloads blocked, directory sites refuse or flag the listing | code signing, VirusTotal checks before every release, vendor false-positive reports, plain explanation in /faq | 1d per release cycle until signed |
| Misuse framing (people asking about games or anti-cheat) | brand damage, store or directory removal | copy never mentions games; replies point to desktop task use; do not add features aimed at evading detection | ongoing |
| Wayland is now the default on major distros | Linux users bounce | honest /linux page with the X11 check; Wayland work on the 365-day plan | see plan |
| r/opensource rules on project age, contributors and AI | post removal, account flagged | hold until eligible (community.md 2d) | 0 |
| macOS permission friction (Accessibility plus Input Monitoring, non-notarized first launch) | install drop-off | screenshots on /macos, notarization in 90 days | 3d |
| Single maintainer | slow issue response hurts stars and reviews | issue templates, a triage hour per week | 0.5d setup |
| GitHub release edge returning 504 right after a release (memory note 11-09) | failed installs at launch moments | publish releases a day before any launch post; installers already retry | 0 |
| Competitor parity (CrossMacro on Wayland, Pulover's editor) | comparison pages age | review /compare quarterly against their current releases | 0.5d per quarter |
| Speed feature copy posted before v0.1.3 ships | false claims in public | `[v0.1.3]` gate in listings.md section 0 | 0 |

## 8. Decisions 2026-11-09

Defaults set by the orchestrator while the owner is silent. The owner can override any of them.

1. Playback speed copy stays behind the `[v0.1.3]` gate. Recording history (F4 panel, list of past recordings) ships in v0.1.4 and gets its own `[v0.1.4]` gate (listings.md section 0, gate 2a).
2. No post goes out until mouserec.com passes live checks. The site stream owns the deploy.
3. r/opensource: hold.
4. r/macapps: monthly App Pile thread only. No LinkedIn trust route.
5. Cross-promotion of other B4Battle products: none in this kit (section 2 is parked).
6. Pro as an open core add-on or a licence key in the MIT tree: open owner decision. Nothing in this plan depends on it; the Pro rows in section 1 are options, not commitments.
