#!/usr/bin/env python3
"""Run a cargo build, test, clippy or fmt of a git ref on a worker machine (win2080, pc3, the Mac mini, or linux: WSL on win2080).

  python tools/remote-build.py --where win2080 --ref origin/main -- cargo test
  python tools/remote-build.py --where mac --ref HEAD -- cargo clippy --all-targets -- -D warnings
  python tools/remote-build.py --where pc3 --ref HEAD --fetch release/mouse-keys-record-replay.exe -- cargo build --release
  python tools/remote-build.py --where mac --ref origin/main --universal

Exit code is cargo's. 75 means nothing was started (worker busy, unreachable, or a quiet timing lock),
124 a timeout, 4 a failed --fetch. See tools/REMOTE-BUILD.md.

Stdlib only. The transport and the busy tests are the shared offload tools, imported and never changed:
REPRISE_OFFLOAD_DIR points at their folder (default D:\\sv-wt\\r6-winworker\\tools\\offload).
"""
import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import threading
import time
from pathlib import Path

OFFLOAD = Path(os.environ.get("REPRISE_OFFLOAD_DIR", r"D:\sv-wt\r6-winworker\tools\offload"))
EXIT_BUSY, EXIT_TIMEOUT, EXIT_FETCH = 75, 124, 4
KEEP_SOURCES = 3
BIN = "mouse-keys-record-replay"
# ssh, tar and git are console programs: without this each one opens a terminal window when the
# caller has no console of its own (an agent shell, a scheduled task).
NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}
if os.name == "nt":
    # The shared offload tools start ssh themselves; every child of this process gets no window.
    _popen_init = subprocess.Popen.__init__

    def _no_window(self, *a, **k):
        k["creationflags"] = k.get("creationflags", 0) | subprocess.CREATE_NO_WINDOW
        _popen_init(self, *a, **k)
    subprocess.Popen.__init__ = _no_window
WIN_ROOT = r"C:\ProgramData\StageVisualsWorker\reprise"
WIN_FLEET = r"C:\ProgramData\StageVisualsWorker\lanes\fleet"
MAC_BUILD_DIR = "reprise-build"
MAC_MIN_MEM_GB, WIN_MIN_DISK_GB = 7, 30
# A release build fits in about 6 GB; tools/release-local.py lowers the floor and cleans after the fetch.
MAC_MIN_DISK_GB = int(os.environ.get("REPRISE_MAC_MIN_DISK_GB", "20"))
MAC_JOBS = 3  # under the Mac offload runner's own CPU busy threshold
DARWIN = ("aarch64-apple-darwin", "x86_64-apple-darwin")
WORKERS = ("win2080", "pc3", "mac", "linux")
# linux = the Ubuntu 22.04 WSL distro on win2080 (see tools/REMOTE-BUILD.md for how it was made)
WSL_DISTRO, WSL_USER = "Ubuntu-22.04", "build"
WSL_B = "/home/build/reprise-build"

TOKEN = re.compile(r"[A-Za-z0-9_./:=+,-]+")
LINT = re.compile(r"[a-z][a-z0-9_-]*(::[a-z][a-z0-9_-]*)?")
REL_PATH = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.-]*(/[A-Za-z0-9_][A-Za-z0-9_.-]*)*")
SWITCHES = {"--release", "--locked", "--all-targets", "--no-run", "--lib", "--bins", "--tests", "--no-fail-fast",
            "--all-features", "--no-default-features", "--check", "--all", "--workspace", "-q", "--quiet", "-v", "--verbose"}
VALUE_FLAGS = {"-p", "--package", "--features", "--bin", "--test", "-j", "--jobs", "--target"}
HARNESS = {"--nocapture", "--exact", "--ignored", "--include-ignored", "--show-output"}


def say(msg):
    print("remote-build: " + msg, file=sys.stderr, flush=True)


def check_cargo(cmd, where):
    """Only build, test, clippy and fmt, with plain flags: the line reaches a remote shell. Returns the words after `cargo`."""
    if len(cmd) < 2 or cmd[0] != "cargo":
        raise SystemExit("remote-build: the command after -- must be: cargo <build|test|clippy|fmt> ...")
    for w in cmd:
        if not TOKEN.fullmatch(w):
            raise SystemExit("remote-build: refused argument %r" % w)
    sub, rest = cmd[1], cmd[2:]
    if sub not in ("build", "test", "clippy", "fmt"):
        raise SystemExit("remote-build: only cargo build, test, clippy and fmt are allowed (nothing is ever run)")
    before, after = (rest[:rest.index("--")], rest[rest.index("--") + 1:]) if "--" in rest else (rest, [])
    allowed_target = {"mac": DARWIN, "linux": ("x86_64-unknown-linux-gnu",)}.get(where, ("x86_64-pc-windows-msvc",))
    i = 0
    while i < len(before):
        w = before[i]
        if w in SWITCHES:
            i += 1
        elif w in VALUE_FLAGS and i + 1 < len(before):
            if w == "--target" and before[i + 1] not in allowed_target:
                raise SystemExit("remote-build: --target must be one of %s on %s" % (", ".join(allowed_target), where))
            if w in ("-j", "--jobs") and not before[i + 1].isdigit():
                raise SystemExit("remote-build: --jobs takes a number")
            i += 2
        else:
            raise SystemExit("remote-build: flag not allowed: %s" % w)
    i = 0
    while i < len(after):
        w = after[i]
        if sub == "clippy" and re.fullmatch(r"-[DWAF]", w) and i + 1 < len(after) and LINT.fullmatch(after[i + 1]):
            i += 2
        elif sub == "clippy" and re.fullmatch(r"-[DWAF][a-z][a-z0-9_:-]*", w):
            i += 1
        elif sub == "fmt" and w == "--check":
            i += 1
        elif sub == "test" and (w in HARNESS or re.fullmatch(r"--test-threads=[0-9]+", w) or (w == "--skip" and i + 1 < len(after))):
            i += 2 if w == "--skip" else 1
        elif sub == "test" and re.fullmatch(r"[A-Za-z0-9_:]+", w):
            i += 1  # a test name filter
        else:
            raise SystemExit("remote-build: argument after -- not allowed: %s" % w)
    return cmd[1:]


def run_local(*argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **NO_WINDOW, **kw)


def resolve(repo, ref):
    got = run_local("git", "-C", repo, "rev-parse", "--verify", ref + "^{commit}")
    if got.returncode:
        raise SystemExit("remote-build: unknown ref %s (git fetch first)" % ref)
    return got.stdout.strip()


def toplevel(repo):
    return run_local("git", "-C", repo, "rev-parse", "--show-toplevel").stdout.strip()


def branch_key(ref):
    return re.sub(r"[^a-z0-9]+", "_", ref.lower()).strip("_")[:40] or "x"


def shipper_alive(pid):
    """A lock names the PID of the python on this PC that holds it; no python under that PID means a lost run."""
    got = run_local("tasklist", "/FI", "PID eq %d" % pid, "/FO", "CSV", "/NH") if os.name == "nt" else None
    if got is None:
        try:
            os.kill(pid, 0)
            return True
        except OSError:
            return False
    if got.returncode:
        return True
    return got.stdout.strip().split(",")[0].strip('"').lower() in ("python.exe", "python3.exe", "py.exe", "pythonw.exe")


class Busy(Exception):
    pass


class Windows:
    """win2080 / pc3: the shared fleet toolchain (read only), this tool's own folder next to the lanes."""

    def __init__(self, where):
        sys.path.insert(0, str(OFFLOAD))
        import dispatch
        import remote_test2 as rt2
        self.rt2, self.where = rt2, where
        t = rt2.TARGETS[where]
        dispatch.WIN_PORT = t["port"]
        self.lock_name = t["lock"]
        os.environ.pop("SV_OFFLOAD_WIN_LAN", None)
        rt2.remote_argv.lan = None  # the LAN key is not valid for these two; always the box tunnel
        self.root = WIN_ROOT
        self.cap = rt2.capture

    def argv(self, line):
        return self.rt2.remote_argv(line)

    def dirs(self, sub):
        got = self.cap('dir /b /ad /o-d "%s\\%s" 2>nul' % (self.root, sub))
        if got.returncode == 255:
            raise Busy("worker unreachable")
        return got.stdout.split()

    def setup(self):
        r = self.root
        for d in ("src", "target", "tmp", "cargo"):
            self.cap('if not exist "%s\\%s" mkdir "%s\\%s"' % (r, d, r, d))
        if not self.rt2.exists(WIN_FLEET + "\\low.cmd"):
            say("the fleet toolchain is missing on %s; run remote_test2.py --setup there first" % self.where)
            return 1
        got = self.cap('call "%s\\env.cmd" && "%s\\rust\\cargo\\bin\\cargo.exe" -V && "%s\\rust\\cargo\\bin\\rustfmt.exe" --version' % (WIN_FLEET, WIN_FLEET, WIN_FLEET))
        print(got.stdout.strip())
        if got.returncode:
            say("cargo works; rustfmt is not in the shared toolchain, so `fmt` is refused on %s (use mac)" % self.where)
        print("ready: %s" % self.root)
        return 0

    def free_gb(self):
        return self.rt2.disk_free_gb()

    def wait_free(self, max_wait):
        free = self.free_gb()
        if free is not None and free < WIN_MIN_DISK_GB + 5:
            for name in self.dirs("target")[::-1]:  # oldest first, only this tool's own targets
                if (self.free_gb() or 0) >= WIN_MIN_DISK_GB + 10:
                    break
                self.drop(self.root + "\target\\" + name)
        why = self.rt2.wait_until_free(max_wait, need_disk=True, timing_lock=self.lock_name)
        if why:
            raise Busy(why)

    def drop(self, path):
        self.rt2.drop_dir(path)

    def take_lock(self, max_wait):
        lock = self.root + "\\run.lock"
        end = time.time() + max_wait
        while True:
            got = self.cap('mkdir "%s" 2>nul && (>"%s\\pid" echo %d)' % (lock, lock, os.getpid()))
            if got.returncode == 255:
                raise Busy("worker unreachable")
            if got.returncode == 0:
                return
            holder = self.cap('type "%s\\pid"' % lock).stdout.strip()
            if holder.isdigit() and not shipper_alive(int(holder)):
                self.cap('rmdir /s /q "%s"' % lock)
                continue
            if time.time() >= end:
                raise Busy("another remote-build run holds %s (pid %s)" % (self.where, holder or "unknown"))
            say("waiting, another remote-build run on %s" % self.where)
            time.sleep(20)

    def drop_lock(self):
        self.cap('rmdir /s /q "%s\\run.lock" 2>nul' % self.root)

    def ship(self, sha, repo):
        src = "%s\\src\\%s" % (self.root, sha[:16])
        if self.rt2.exists(src + "\\Cargo.toml"):
            return src
        self.prune_src(keep=sha[:16])
        self.drop(src)
        tmp = "%s.%d" % (src, os.getpid())
        if self.cap('mkdir "%s"' % tmp).returncode:
            raise Busy("cannot create " + tmp)
        try:
            # -m: files get the time of extraction. git archive stamps the commit time, and an older commit
            # shipped into a reused target dir would otherwise look unchanged to cargo.
            git = subprocess.Popen(["git", "-c", "core.autocrlf=false", "-C", toplevel(repo), "archive", "--format=tar", sha], stdout=subprocess.PIPE, **NO_WINDOW)
            rc = subprocess.run(self.argv("tar xmf - -C %s" % tmp.replace("\\", "/")), stdin=git.stdout, **NO_WINDOW).returncode
            git.stdout.close()
            if git.wait() or rc:
                raise RuntimeError("shipping the source failed (local %s, worker %s)" % (git.returncode, rc))
            self.cap('if not exist "%s" move "%s" "%s" >nul' % (src, tmp, src))
        finally:
            self.cap('if exist "%s" rmdir /s /q "%s"' % (tmp, tmp))
        if not self.rt2.exists(src + "\\Cargo.toml"):
            raise RuntimeError("source folder did not land: " + src)
        return src

    def prune_src(self, keep):
        names = self.dirs("src")
        for n in names:
            if n.endswith(".del") or ("." in n and n.rpartition(".")[2].isdigit() and not shipper_alive(int(n.rpartition(".")[2]))):
                self.cap('rmdir /s /q "%s\\src\\%s"' % (self.root, n), timeout=600)
        full = [n for n in names if "." not in n and n != keep]
        for n in full[KEEP_SOURCES - 1:]:
            self.drop("%s\\src\\%s" % (self.root, n))

    def touch(self, src):
        self.cap('del "%s\\.last-run" 2>nul & type nul > "%s\\.last-run"' % (src, src))

    def line(self, src, key, words):
        r = self.root
        cargo = "%s\\rust\\cargo\\bin\\cargo.exe" % WIN_FLEET
        sets = " && ".join('set "%s=%s"' % kv for kv in (
            ("TEMP", r + "\\tmp"), ("TMP", r + "\\tmp"), ("CARGO_HOME", r + "\\cargo"),
            ("CARGO_TARGET_DIR", "%s\\target\\%s" % (r, key)), ("CARGO_INCREMENTAL", "0"), ("CARGO_TERM_COLOR", "never")))
        # winresource finds rc.exe through the registry, which the shared lane has no SDK entry in; with no entry it
        # falls back to bin\x64\rc.exe under the package folder, so the lane's own copy is placed there.
        rc = ('(if not exist "bin\\x64\\rc.exe" for /d %%d in ("%s\\sdk\\bin\\10.*") do '
              '(xcopy /y /i /q "%%d\\x64\\rc.exe" "bin\\x64\\" >nul & xcopy /y /q "%%d\\x64\\rcdll.dll" "bin\\x64\\" >nul))' % WIN_FLEET)
        return ('call "%s\\env.cmd" && %s && (cd /d "%s" || exit 75) && %s && "%s\\low.cmd" "%s" %s'
                % (WIN_FLEET, sets, src, rc, WIN_FLEET, cargo, " ".join(words)))

    def stop(self):
        """End what this tool started on the worker, found by its folder in the path or command line; never by image name.
        The pattern is built in two pieces so it does not match this very command line."""
        got = self.cap("powershell -NoProfile -NonInteractive -Command \"$m='%s\\' + '*'; "
                       "$f={ Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.CommandLine -like ('*'+$m) -or $_.ExecutablePath -like $m } }; "
                       "& $f | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; "
                       "Start-Sleep 2; exit [int](@(& $f).Count -gt 0)\"" % self.root)
        if got.returncode:
            say("WARNING the run may still be going on %s" % self.where)

    def fetch(self, key, rel, dest):
        path = "%s\\target\\%s\\%s" % (self.root, key, rel.replace("/", "\\"))
        got = self.cap('certutil -hashfile "%s" MD5' % path)
        lines = [l.strip().replace(" ", "") for l in got.stdout.splitlines()]
        md5 = next((l for l in lines if re.fullmatch(r"[0-9a-f]{32}", l)), None)
        if not md5:
            return None
        base = "%s/target/%s" % (self.root.replace("\\", "/"), key)
        return self.argv("tar cf - -C %s %s" % (base, rel)), md5


class Mac:
    """The M6 Mac mini: CI first, then this. Everything under ~/reprise-build, nothing system wide."""

    def __init__(self):
        sys.path.insert(0, str(OFFLOAD))
        import dispatch
        self.d = dispatch
        self.where = "mac"
        self.b = "$HOME/" + MAC_BUILD_DIR

    def argv(self, cmd):
        return self.d.mac_argv(cmd)

    def sh(self, cmd, stdin=None, timeout=120, check=False):
        try:
            return self.d.mac(cmd, stdin=stdin, timeout=timeout, check=check).decode(errors="replace")
        except self.d.MacUnreachable as e:
            raise Busy("Mac unreachable: %s" % e)

    PROBE = r'''
import os, re, shutil, subprocess, json
ps = subprocess.run(["ps", "-axo", "comm="], capture_output=True, text=True).stdout.split("\n")
names = sorted({p.rsplit("/", 1)[-1].strip() for p in ps})
busy = [n for n in names if re.fullmatch(r"(Runner\.Worker|xcodebuild|stagevisuals.*)", n, re.I)]
vm = subprocess.run(["vm_stat"], capture_output=True, text=True).stdout
size = int(re.search(r"page size of (\d+)", vm).group(1))
pages = lambda k: int((re.search(k + r":\s+(\d+)", vm) or [0, 0])[1])
mem = (pages("Pages free") + pages("Pages inactive") + pages("Pages speculative") + pages("Pages purgeable")) * size / 2**30
disk = shutil.disk_usage(os.path.expanduser("~")).free / 2**30
print(json.dumps({"busy": busy, "mem_gb": round(mem, 1), "disk_gb": round(disk, 1)}))
'''

    def probe(self):
        return json.loads(self.sh("python3 -", stdin=self.PROBE.encode()))

    def why_busy(self):
        p = self.probe()
        if p["busy"]:
            return "CI or another build is running on the Mac: %s" % ",".join(p["busy"])
        if p["mem_gb"] < MAC_MIN_MEM_GB:
            return "Mac memory low: %s GB available, need %d" % (p["mem_gb"], MAC_MIN_MEM_GB)
        if p["disk_gb"] < MAC_MIN_DISK_GB:
            return "Mac disk low: %s GB free, need %d" % (p["disk_gb"], MAC_MIN_DISK_GB)
        return None

    def wait_free(self, max_wait):
        end = time.time() + max_wait
        while True:
            why = self.why_busy()
            if not why:
                return
            if time.time() >= end:
                raise Busy(why)
            say("waiting, %s" % why)
            time.sleep(30)

    WRAPPER = r'''#!/bin/sh
# usage: run.sh <target key> <source folder> <cargo words...> | universal
B="$HOME/reprise-build"
# This tool's own rustup when --setup installed it, else the user's existing ~/.cargo + ~/.rustup.
if [ -x "$B/cargo/bin/cargo" ]; then
  export RUSTUP_HOME="$B/rustup" CARGO_HOME="$B/cargo"
else
  export RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo"
fi
export TMPDIR="$B/tmp"
export PATH="$CARGO_HOME/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_TARGET_DIR="$B/target/$1" CARGO_INCREMENTAL=0 CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS="${REPRISE_JOBS:-3}" MACOSX_DEPLOYMENT_TARGET=11.0
cd "$B/src/$2" || exit 75
shift 2
set -m
if [ "$1" = universal ]; then
  nice -n 10 sh -c 'set -e
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    mkdir -p "$CARGO_TARGET_DIR/universal"
    lipo -create -output "$CARGO_TARGET_DIR/universal/mouse-keys-record-replay" \
      "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/mouse-keys-record-replay" \
      "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/mouse-keys-record-replay"
    lipo -info "$CARGO_TARGET_DIR/universal/mouse-keys-record-replay"
    sh packaging/macos/bundle.sh "$CARGO_TARGET_DIR/universal/mouse-keys-record-replay" "$CARGO_TARGET_DIR/dist"
    codesign --verify --verbose "$CARGO_TARGET_DIR/dist/Mouse Keys Record Replay.app"' &
else
  nice -n 10 "$@" &
fi
pid=$!
trap 'kill -CONT -- -$pid 2>/dev/null; kill -TERM -- -$pid 2>/dev/null; wait $pid 2>/dev/null; exit 143' HUP TERM INT
paused=0
while kill -0 $pid 2>/dev/null; do
  if ps -axo comm= | awk -F/ '{print $NF}' | grep -Eiq '^(Runner\.Worker|xcodebuild|stagevisuals.*)$'; then
    if [ $paused = 0 ]; then echo "remote-build: CI or another build started on the Mac, paused" >&2; kill -STOP -- -$pid; paused=1; fi
  elif [ $paused = 1 ]; then
    echo "remote-build: resumed" >&2; kill -CONT -- -$pid; paused=0
  fi
  sleep 2
done
wait $pid
exit $?
'''

    def setup(self):
        b = self.b
        self.sh("mkdir -p %s/rustup %s/cargo %s/src %s/target %s/tmp" % ((b,) * 5), check=True)
        self.sh("cat > %s/run.sh" % b, stdin=self.WRAPPER.encode(), check=True)
        have = self.sh('RUSTUP_HOME=%s/rustup CARGO_HOME=%s/cargo %s/cargo/bin/rustup target list --installed 2>/dev/null' % (b, b, b))
        if all(t in have for t in DARWIN):
            print("rust already installed:\n" + self.sh("%s/cargo/bin/cargo -V" % b))
            return 0
        user = self.sh('RUSTUP_HOME=$HOME/.rustup CARGO_HOME=$HOME/.cargo $HOME/.cargo/bin/rustup target list --installed 2>/dev/null')
        if all(t in user for t in DARWIN):
            print("using the existing ~/.cargo toolchain:\n" + self.sh("RUSTUP_HOME=$HOME/.rustup $HOME/.cargo/bin/cargo -V"))
            return 0
        p = self.probe()
        if p["disk_gb"] < MAC_MIN_DISK_GB + 3:
            say("Mac disk is %s GB free; the install needs about 3 GB above the %d GB floor" % (p["disk_gb"], MAC_MIN_DISK_GB))
            return EXIT_BUSY
        say("installing rustup into ~/%s (nothing system wide)" % MAC_BUILD_DIR)
        out = self.sh(
            'cd %s && curl -fsSL https://sh.rustup.rs -o rustup-init.sh && RUSTUP_HOME=%s/rustup CARGO_HOME=%s/cargo '
            'nice -n 10 sh rustup-init.sh -y --no-modify-path --profile minimal -t aarch64-apple-darwin -t x86_64-apple-darwin '
            '-c clippy -c rustfmt 2>&1 | tail -15' % (b, b, b), timeout=1500)
        print(out)
        have = self.sh('RUSTUP_HOME=%s/rustup CARGO_HOME=%s/cargo %s/cargo/bin/rustup target list --installed' % (b, b, b))
        return 0 if all(t in have for t in DARWIN) else 1

    def take_lock(self, max_wait):
        end = time.time() + max_wait
        while True:
            got = self.sh('mkdir %s/run.lock 2>/dev/null && echo %d > %s/run.lock/pid && echo got' % (self.b, os.getpid(), self.b))
            if "got" in got:
                return
            holder = self.sh("cat %s/run.lock/pid 2>/dev/null" % self.b).strip()
            if holder.isdigit() and not shipper_alive(int(holder)):
                self.sh("rm -rf %s/run.lock" % self.b)
                continue
            if time.time() >= end:
                raise Busy("another remote-build run holds the Mac (pid %s)" % (holder or "unknown"))
            say("waiting, another remote-build run on the Mac")
            time.sleep(20)

    def drop_lock(self):
        self.sh("rm -rf %s/run.lock" % self.b)

    def ship(self, sha, repo):
        name = sha[:16]
        src = "%s/src/%s" % (self.b, name)
        self.sh("mkdir -p %s/src %s/target %s/tmp && cat > %s/run.sh" % ((self.b,) * 4), stdin=self.WRAPPER.encode(), check=True)
        if "ok" in self.sh("test -f %s/Cargo.toml && echo ok" % src):
            return name
        self.sh("rm -rf %s.part" % src)
        git = subprocess.Popen(["git", "-c", "core.autocrlf=false", "-C", toplevel(repo), "archive", "--format=tar.gz", sha], stdout=subprocess.PIPE, **NO_WINDOW)
        # -m: extraction time, see Windows.ship
        rc = subprocess.run(self.argv("mkdir -p %s.part && tar xmzf - -C %s.part && mv %s.part %s" % (src, src, src, src)), stdin=git.stdout, **NO_WINDOW).returncode
        git.stdout.close()
        if git.wait() or rc:
            raise RuntimeError("shipping the source failed (local %s, mac %s)" % (git.returncode, rc))
        return name

    def prune_src(self, keep):
        self.sh("cd %s/src && ls -t | grep -v '^%s$' | tail -n +%d | while read d; do rm -rf \"$d\"; done" % (self.b, keep, KEEP_SOURCES))

    def touch(self, name):
        self.sh("touch %s/src/%s/.last-run && touch %s/src/%s" % (self.b, name, self.b, name))

    def line(self, name, key, words):
        words = words if words == ["universal"] else ["cargo"] + words
        return "sh %s/run.sh %s %s %s" % (self.b, key, name, " ".join(shlex.quote(w) for w in words))

    def stop(self):
        self.sh("pkill -TERM -f '%s/run.sh'; true" % MAC_BUILD_DIR)

    def fetch(self, key, rel, dest):
        path = "%s/target/%s/%s" % (self.b, key, rel)
        md5 = self.sh("md5 -q %s 2>/dev/null" % path).strip()
        if not re.fullmatch(r"[0-9a-f]{32}", md5):
            return None
        return self.argv("tar cf - -C %s/target/%s %s" % (self.b, key, rel)), md5


class Linux(Windows):
    """The Ubuntu 22.04 WSL distro on win2080, user build: rustup in ~/.cargo, this tool's files in ~/reprise-build.
    Reached through win2080's cmd.exe, so it shares that worker's quiet-lock and disk checks."""

    WRAPPER = r'''#!/bin/sh
# usage: run.sh <target key> <source folder> <cargo words...>
B=%s
export CARGO_HOME=$HOME/.cargo RUSTUP_HOME=$HOME/.rustup TMPDIR=$B/tmp
export PATH=$HOME/.cargo/bin:/usr/bin:/bin CARGO_TARGET_DIR=$B/target/$1 CARGO_INCREMENTAL=0 CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=${REPRISE_JOBS:-12}
cd $B/src/$2 || exit 75
shift 2
exec nice -n 10 "$@"
''' % WSL_B

    def __init__(self):
        super().__init__("win2080")
        self.where, self.b = "linux", WSL_B

    @staticmethod
    def wsl(cmd):
        # cmd reaches cmd.exe inside double quotes: no double quotes or % in it
        return 'wsl -d %s -u %s -e sh -c "%s"' % (WSL_DISTRO, WSL_USER, cmd)

    def sh(self, cmd, timeout=120):
        got = self.cap(self.wsl(cmd), timeout=timeout)
        if got.returncode == 255:
            raise Busy("worker unreachable")
        return got

    def setup(self):
        got = self.sh("mkdir -p %s/src %s/target %s/tmp && $HOME/.cargo/bin/cargo -V && pkg-config --modversion gtk+-3.0" % ((self.b,) * 3))
        print(got.stdout.strip() or got.stderr.strip())
        return got.returncode

    def wait_free(self, max_wait):
        why = self.rt2.wait_until_free(max_wait, need_disk=True, timing_lock=self.lock_name)
        if why:
            raise Busy(why)

    def take_lock(self, max_wait):
        end = time.time() + max_wait
        while True:
            if "got" in self.sh("mkdir %s/run.lock 2>/dev/null && echo %d > %s/run.lock/pid && echo got" % (self.b, os.getpid(), self.b)).stdout:
                return
            holder = self.sh("cat %s/run.lock/pid 2>/dev/null" % self.b).stdout.strip()
            if holder.isdigit() and not shipper_alive(int(holder)):
                self.sh("rm -rf %s/run.lock" % self.b)
                continue
            if time.time() >= end:
                raise Busy("another remote-build run holds linux (pid %s)" % (holder or "unknown"))
            say("waiting, another remote-build run on linux")
            time.sleep(20)

    def drop_lock(self):
        self.sh("rm -rf %s/run.lock" % self.b)

    def ship(self, sha, repo):
        name = sha[:16]
        src = "%s/src/%s" % (self.b, name)
        put = subprocess.run(self.argv(self.wsl("mkdir -p %s/src %s/target %s/tmp && cat > %s/run.sh" % ((self.b,) * 4))),
                             input=self.WRAPPER.encode(), capture_output=True, **NO_WINDOW)
        if put.returncode:
            raise RuntimeError("could not write run.sh on linux")
        if "ok" in self.sh("test -f %s/Cargo.toml && echo ok" % src).stdout:
            return name
        self.sh("rm -rf %s.part" % src)
        git = subprocess.Popen(["git", "-c", "core.autocrlf=false", "-C", toplevel(repo), "archive", "--format=tar", sha], stdout=subprocess.PIPE, **NO_WINDOW)
        rc = subprocess.run(self.argv(self.wsl("mkdir -p %s.part && tar xmf - -C %s.part && mv %s.part %s" % (src, src, src, src))),
                            stdin=git.stdout, **NO_WINDOW).returncode
        git.stdout.close()
        if git.wait() or rc:
            raise RuntimeError("shipping the source failed (local %s, linux %s)" % (git.returncode, rc))
        return name

    def prune_src(self, keep):
        self.sh("cd %s/src && ls -t | grep -v '^%s$' | tail -n +%d | while read d; do rm -rf $d; done" % (self.b, keep, KEEP_SOURCES))

    def touch(self, name):
        self.sh("touch %s/src/%s/.last-run %s/src/%s" % (self.b, name, self.b, name))

    def line(self, name, key, words):
        return "wsl -d %s -u %s -e sh %s/run.sh %s %s cargo %s" % (WSL_DISTRO, WSL_USER, self.b, key, name, " ".join(words))

    def stop(self):
        # the distro's build user runs nothing but these builds
        self.cap('wsl -d %s -u root -e pkill -TERM -u %s -f "cargo|rustc|reprise-build"' % (WSL_DISTRO, WSL_USER))

    def fetch(self, key, rel, dest):
        got = self.sh("md5sum %s/target/%s/%s" % (self.b, key, rel)).stdout.split()
        md5 = got[0] if got and re.fullmatch(r"[0-9a-f]{32}", got[0]) else None
        if not md5:
            return None
        return self.argv(self.wsl("tar cf - -C %s/target/%s %s" % (self.b, key, rel))), md5


def stream(argv, timeout, stop):
    p = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, encoding="utf-8", errors="replace", **NO_WINDOW)
    hit = []

    def fire():
        hit.append(True)
        try:
            stop()
        finally:
            p.kill()

    timer = threading.Timer(timeout, fire)
    timer.start()
    passed = 0
    try:
        for line in p.stdout:
            print(line, end="", flush=True)
            m = re.match(r"test result: \w+\. (\d+) passed", line)
            passed += int(m.group(1)) if m else 0
        return p.wait(), passed, bool(hit)
    finally:
        timer.cancel()


def do_fetch(w, key, rel, dest):
    if not REL_PATH.fullmatch(rel):
        say("--fetch takes a plain relative path inside the target folder")
        return EXIT_FETCH
    got = w.fetch(key, rel, dest)
    if not got:
        say("nothing to fetch at %s" % rel)
        return EXIT_FETCH
    argv, md5 = got
    dest = Path(dest)
    dest.mkdir(parents=True, exist_ok=True)
    tar = subprocess.run(argv, capture_output=True, **NO_WINDOW)
    if tar.returncode or not tar.stdout:
        say("fetch failed: %s" % tar.stderr.decode(errors="replace")[-300:])
        return EXIT_FETCH
    import io
    import tarfile
    with tarfile.open(fileobj=io.BytesIO(tar.stdout)) as t:
        t.extractall(dest, filter="data")
    got_md5 = hashlib.md5((dest / rel).read_bytes()).hexdigest()
    if got_md5 != md5:
        say("fetch md5 mismatch: worker %s, here %s" % (md5, got_md5))
        return EXIT_FETCH
    say("fetched %s (md5 %s) to %s" % (rel, md5, dest))
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(prog="remote-build.py", description=__doc__.split("\n\n")[0])
    ap.add_argument("--where", required=True, choices=WORKERS)
    ap.add_argument("--ref", default="HEAD", help="git ref to build (default HEAD)")
    ap.add_argument("--repo", default=".", help="checkout to take the ref from (default .)")
    ap.add_argument("--fetch", metavar="REL", help="copy REL from the worker's target folder (for example release/%s.exe)" % BIN)
    ap.add_argument("--to", default="remote-out", help="folder for --fetch (default ./remote-out)")
    ap.add_argument("--max-wait", type=int, default=300, help="seconds to wait for a free worker before exit 75")
    ap.add_argument("--timeout", type=int, default=3000, help="seconds for the cargo run before it is stopped")
    ap.add_argument("--target-key", help="target folder name on the worker (default: from --ref)")
    ap.add_argument("--setup", action="store_true", help="prepare this tool's folder on the worker (the Mac also gets rustup)")
    ap.add_argument("--clean", action="store_true", help="delete this tool's folders on the worker and nothing else")
    ap.add_argument("--universal", action="store_true", help="mac only: the release.yml universal build (both targets, then lipo)")
    ap.add_argument("cmd", nargs="*", help="after --: cargo build|test|clippy|fmt ...")
    a = ap.parse_args(argv)
    sys.stdout.reconfigure(errors="replace")
    w = Mac() if a.where == "mac" else Linux() if a.where == "linux" else Windows(a.where)
    try:
        return go(a, w)
    except Busy as e:
        say("not started, %s. Use another worker." % e)
        return EXIT_BUSY
    except (RuntimeError, OSError, subprocess.SubprocessError) as e:
        say("not started: %s" % e)
        return EXIT_BUSY


def go(a, w):
    if a.setup:
        return w.setup()
    if a.clean:
        return clean(w)
    if a.universal:
        if a.where != "mac":
            raise SystemExit("remote-build: --universal is for --where mac")
        words = ["universal"]
    else:
        words = check_cargo(a.cmd, a.where)
        if words[0] == "fmt" and a.where != "mac":
            raise SystemExit("remote-build: fmt runs on the Mac only (the shared Windows toolchain has no rustfmt)")
    sha = resolve(a.repo, a.ref)
    key = a.target_key or branch_key(a.ref)
    if not re.fullmatch(r"[a-z0-9_]{1,40}", key):
        raise SystemExit("remote-build: --target-key must be lowercase letters, digits and _")
    began = time.time()
    w.wait_free(a.max_wait)
    w.take_lock(max(0, a.max_wait - (time.time() - began)))
    try:
        src = w.ship(sha, a.repo)
        w.touch(src)
        w.prune_src(keep=sha[:16])
        w.wait_free(max(0, a.max_wait - (time.time() - began)))  # the copy took time; look again
        t0 = time.time()
        rc, passed, timed_out = stream(w.argv(w.line(src, key, words)), a.timeout, w.stop)
        if timed_out:
            say("stopped after %d s" % a.timeout)
            rc = EXIT_TIMEOUT
        elif rc == 255:
            say("link to %s dropped, result unknown" % a.where)
            w.stop()
        elif rc == 0 and words[0] == "test" and "--no-run" not in words and not passed:
            say("exit 0 but no test ran, reported as a failure")
            rc = 3
        if rc == 0 and a.fetch:
            rc = do_fetch(w, key, a.fetch, a.to)
        say("%s at %s on %s, exit %d, %.0f s" % (" ".join(["cargo"] + words), sha[:9], a.where, rc, time.time() - t0))
        return rc
    finally:
        w.drop_lock()


def clean(w):
    """Only this tool's own folders: the Windows reprise folder, or ~/reprise-build/{src,target,tmp} on the Mac."""
    if isinstance(w, Linux):
        w.sh("cd %s && rm -rf src target tmp run.lock && mkdir src target tmp" % w.b)
        print("cleaned %s/{src,target,tmp} in WSL %s" % (w.b, WSL_DISTRO))
        return 0
    if isinstance(w, Mac):
        w.sh("cd %s && rm -rf src target tmp run.lock && mkdir src target tmp" % w.b)
        print("cleaned ~/%s/{src,target,tmp}; rustup and cargo kept" % MAC_BUILD_DIR)
        return 0
    for sub in ("src", "target", "tmp"):
        for n in w.dirs(sub):
            w.drop("%s\\%s\\%s" % (w.root, sub, n))
    w.drop_lock()
    print("cleaned %s\\{src,target,tmp}" % w.root)
    return 0


if __name__ == "__main__":
    sys.exit(main())
