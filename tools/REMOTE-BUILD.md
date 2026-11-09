# Remote build and test

`tools/remote-build.py` builds and tests a git ref on one of the worker machines
(win2080, pc3, the M6 Mac mini, and an Ubuntu 22.04 WSL distro on win2080) so nothing heavy runs on the developer PC. Standard library only.

```
python tools/remote-build.py --where win2080|pc3|mac|linux --ref <ref> [--repo PATH] -- cargo <build|test|clippy|fmt> ...
```

Examples:

```
python tools/remote-build.py --where pc3 --ref origin/main -- cargo test
python tools/remote-build.py --where win2080 --ref HEAD -- cargo clippy --all-targets -- -D warnings
python tools/remote-build.py --where mac --ref feat/x -- cargo build --release
python tools/remote-build.py --where mac --ref origin/main --universal -- cargo build --release
python tools/remote-build.py --where mac --ref HEAD -- cargo fmt -- --check
```

The ref is resolved locally, shipped with `git archive` (committed content only, never the working tree),
built, and the output is streamed back. The exit code is cargo's own.

## Options

| Option | Meaning |
| --- | --- |
| `--where` | `win2080`, `pc3`, `mac` or `linux` |
| `--ref` | any ref the local repo resolves (default `HEAD`) |
| `--repo` | repo path (default: the repo this file lives in) |
| `--setup` | one-time check/install of the worker side, then exit |
| `--clean` | remove this tool's source and target folders on that worker |
| `--universal` | Mac only: build `aarch64` and `x86_64`, then `lipo -create`, `bundle.sh` and `codesign --verify` as release.yml does (gives `dist/mouse-keys-record-replay-macos.zip`) |
| `--fetch REL` | after a successful run, copy `REL` from that run's target folder into `--to` |
| `--max-wait` | seconds to wait for the worker to be free (default 300) |
| `--timeout` | seconds the cargo command may run (default 3000) |
| `--target-key` | share a target folder across refs (default: one per branch name) |

Only these cargo commands are accepted: `build`, `test`, `clippy`, `fmt`. Every word must be plain
`[A-Za-z0-9_./:=+,-]`; flags come from a fixed allow-list. `--target` is limited to the darwin triples on
the Mac, `x86_64-unknown-linux-gnu` on linux and `x86_64-pc-windows-msvc` on Windows. After `--`: clippy lint levels (`-D`, `-W`, `-A`, `-F`),
test harness flags, and `--check` for fmt. `cargo run` is refused: built binaries are never launched.
`fmt` runs on the Mac only (the shared Windows toolchain has no rustfmt).

## Exit codes

| Code | Meaning |
| --- | --- |
| cargo's own | the command ran |
| 75 | not started: worker busy past `--max-wait`, unreachable, or below its disk/memory floor |
| 124 | `--timeout` hit; the run was stopped on the worker |
| 4 | `--fetch` failed |
| 3 | cargo exited 0 but no test ran |

## Busy rules (never compete with real work)

- Windows: waits through `remote_test2.wait_until_free` (reused from `tools/offload`, read-only): the
  worker's own queue, other work on the PC, owner sessions, and a `quiet` gpulock on that worker's slot
  (a timing run is under way; nothing compiles). Runs at Idle priority. Needs 30 GB free disk; this
  tool prunes only its own oldest target folders when under 35 GB.
- Mac: GitHub Actions comes first. Does not start (and pauses a run in progress with SIGSTOP) while
  `Runner.Worker`, `xcodebuild` or `stagevisuals` runs. Needs 7 GB free memory and 20 GB free disk (`REPRISE_MAC_MIN_DISK_GB` lowers the disk floor).
  Runs at `nice 10`, 3 build jobs, no incremental.
- One run at a time per worker (`run.lock` folder). A lock whose owner process is gone is cleared.
- The shared offload scripts come from `D:\sv-wt\r6-winworker\tools\offload`; override with the
  environment variable `REPRISE_OFFLOAD_DIR`.

## Where things live

| Worker | Root | Toolchain |
| --- | --- | --- |
| win2080 (tunnel port 22103) | `C:\ProgramData\StageVisualsWorker\reprise` | shared fleet toolchain `...\lanes\fleet` (env.cmd, low.cmd, rust) |
| pc3 (tunnel port 22104) | `C:\ProgramData\StageVisualsWorker\reprise` | same |
| mac | `~/reprise-build` | `~/reprise-build/cargo` when `--setup` installed rustup there, else the existing `~/.cargo` and `~/.rustup` (targets aarch64 and x86_64 darwin); target and tmp stay under `~/reprise-build` |
| linux (WSL on win2080) | `/home/build/reprise-build` in distro `Ubuntu-22.04`, user `build` | rustup stable in `~/.cargo` of user `build`; shares win2080's quiet-lock and disk check |

Under each root: `src\<sha16>` (the 3 newest are kept), `target\<branch key>` (reused between runs, so a
second run of the same branch is warm), `tmp`, `cargo` (CARGO_HOME, Windows). Sources are extracted with
the extraction time as mtime so an older commit shipped into a reused target still rebuilds.

Windows note: the app's `build.rs` embeds an icon with `winresource`, which looks for `rc.exe` in the
registry. The workers have the SDK files but no registry entry, so each run places the lane's `rc.exe`
and `rcdll.dll` under `bin\x64` in the extracted source folder before cargo starts.

## Limits

The Linux build runs in the WSL distro on win2080, never on the tunnel box (it only tunnels through it).
The built app is never run. Nothing outside the roots above is written or deleted.

## Measured times (origin/main, 2026-11-09)

Cold = first run in a fresh target folder, warm = rerun of the same ref. Seconds, wall clock including ship and extract.

| Command | pc3 cold | pc3 warm | win2080 cold | win2080 warm |
| --- | --- | --- | --- | --- |
| `cargo test` | 44 (part of the deps were already built) | 10 | 49 | 7 |
| `cargo clippy --all-targets -- -D warnings` | 40 | 8 | 27 | 8 |
| `cargo build --release` | 86 | 8 | 49 | 11 |

Clippy exits 101 on origin/main: 6 existing lint errors (`src/input/windows.rs:76`, `src/shortcuts.rs:281`,
`src/ui/windows.rs:396,439,907`, `src/ui/hud/windows.rs:370`). The tool works; the code needs the fixes.

## Mac status

The Mac path works end to end (v0.1.3: 29 tests, universal bundle, `codesign --verify` valid). It uses the
existing `~/.cargo` toolchain, so no rustup install is needed. The default free-disk floor is 20 GB; the Mac
usually has about 20 GB, so `release-local.py` sets `REPRISE_MAC_MIN_DISK_GB=12`. Run `--clean` after a fetch.
Source tarballs are made with `git -c core.autocrlf=false archive`, so shell scripts reach the workers with LF
line endings even when the system gitconfig sets `core.autocrlf=true`.

## Linux worker (WSL on win2080)

Set up 2026-11-09, headless, as `svworker` through the tunnel. pc3 has no WSL feature and is left that way.

- Root filesystem: the official `ubuntu-jammy-wsl-amd64-ubuntu22.04lts.rootfs.tar.gz` from
  `cloud-images.ubuntu.com/wsl/jammy/current/`, sha256 checked against that folder's `SHA256SUMS`, kept in
  `C:\ProgramData\StageVisualsWorker\wsl\dl`.
- `wsl --import Ubuntu-22.04 C:\ProgramData\StageVisualsWorker\wsl\Ubuntu-22.04 <tar> --version 2`
- User `build` (`useradd -m -s /bin/bash`, no sudo). `/etc/wsl.conf`: `[user] default=build`,
  `[interop] appendWindowsPath=false`.
- apt: `build-essential pkg-config curl file ca-certificates git xxd libgtk-3-dev libayatana-appindicator3-dev
  libxdo-dev libx11-dev libxi-dev libxtst-dev`.
- rustup `--profile minimal` stable for `build` (cargo 1.99.1 on setup day).
- Check: `python tools/remote-build.py --where linux --setup` prints the cargo and GTK versions.
- Remove it all: `wsl --unregister Ubuntu-22.04` on win2080, then delete `C:\ProgramData\StageVisualsWorker\wsl`.

## Releases without GitHub Actions

`tools/release-local.py <tag>` ships a pushed `v*` tag the way `.github/workflows/release.yml` does, using the
workers above and this PC (no Actions minutes):

```
python tools/release-local.py v0.1.4                        # build, sign, publish, check
python tools/release-local.py v0.1.4 --steps sign,publish   # resume with the builds already in the dist folder
```

- build: pc3 Windows x64, Mac universal zip, linux x64, each after `cargo test --release`. Assets land in
  `mkrr-dist-<tag>` next to the repo (or `--dist`) under the release.yml names; logs in its `logs` folder.
  The Mac's src/target for this tool are removed after the fetch (its disk is tight).
- sign: the Ed25519 key at `%USERPROFILE%\.keys\mkrr-update-ed25519.pem` (`MKRR_SIGNING_KEY` overrides) must
  match `PUBLIC_KEY` in `src/update.rs` at the tag; 3 asset `.sig` files, `SHA256SUMS` (6 lines) and its `.sig`.
  Needs Python `cryptography`.
- publish: git archive snapshot, `.github` reduced to `build.yml`, pushed to the public `main` as TikTool,
  `gh release create --latest` at that commit (local `gh` login, account `tiktooldev`).
- check: install.sh on the Mac (private OpenSSL 3 at `~/reprise-build/openssl3`, then stock LibreSSL) and in
  the WSL distro, install.ps1 on pc3 in Windows PowerShell 5.1, all with `MKRR_NO_LAUNCH=1`. These install the
  app on the workers; nothing is launched.
