#!/usr/bin/env python3
"""Ship a tagged release from our own machines, the way .github/workflows/release.yml does (no Actions minutes).

  python tools/release-local.py v0.1.4                      # build, sign, publish, check
  python tools/release-local.py v0.1.4 --steps sign,publish  # resume from a dist folder that has the builds

Steps:
  build    pc3: cargo test/build --release (Windows x64). mac: cargo test --release, universal + bundle.sh +
           codesign --verify. linux (WSL on win2080): cargo test/build --release. All through tools/remote-build.py,
           in parallel, fetched into the dist folder under the release.yml asset names.
  sign     Ed25519 over the 3 assets and SHA256SUMS with the key on this PC, checked against PUBLIC_KEY in
           src/update.rs at the tag. Needs the Python `cryptography` package.
  publish  git archive of the tag (export-ignore applies), .github reduced to build.yml, pushed to public main as
           "Source for <tag>" by TikTool; gh release create on the public repo at that commit; the public tag must
           resolve to it.
  check    install.sh on the Mac (OpenSSL 3 at ~/reprise-build/openssl3, then stock LibreSSL) and in the WSL
           distro, install.ps1 on pc3 under Windows PowerShell 5.1, all with MKRR_NO_LAUNCH=1, against the release.

Exit code: 0 when every requested step passed, else 1. Logs land in <dist>/logs.
"""
import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
PUBLIC_REPO = "tiktool/mouse-keys-record-replay"
IDENTITY = {"GIT_AUTHOR_NAME": "TikTool", "GIT_AUTHOR_EMAIL": "271127222+tiktooldev@users.noreply.github.com",
            "GIT_COMMITTER_NAME": "TikTool", "GIT_COMMITTER_EMAIL": "271127222+tiktooldev@users.noreply.github.com"}
KEY = Path(os.environ.get("MKRR_SIGNING_KEY", Path.home() / ".keys" / "mkrr-update-ed25519.pem"))
WIN, MAC, LINUX = "mouse-keys-record-replay-windows-x64.exe", "mouse-keys-record-replay-macos.zip", "mouse-keys-record-replay-linux-x64"
ASSETS = (WIN, MAC, LINUX)
NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}
MAC_OPENSSL3 = "$HOME/reprise-build/openssl3/openssl"
WIN_TMP = r"C:\ProgramData\StageVisualsWorker\reprise\tmp"
APP = "Mouse Keys Record Replay.app"


def say(msg):
    print("release-local: " + msg, flush=True)


def run(*argv, check=True, **kw):
    got = subprocess.run(argv, capture_output=True, text=True, encoding="utf-8", errors="replace", **NO_WINDOW, **kw)
    if check and got.returncode:
        raise SystemExit("release-local: %s failed (%d): %s" % (argv[0], got.returncode, got.stderr.strip()[-800:]))
    return got


def git(*args, **kw):
    return run("git", "-C", str(REPO), *args, **kw).stdout


# ---------------------------------------------------------------- build

def remote_build(log, *args):
    """One tools/remote-build.py call; returns (rc, tests passed)."""
    argv = [sys.executable, str(HERE / "remote-build.py"), "--max-wait", "5400", "--timeout", "3600", "--repo", str(REPO), *args]
    with open(log, "w", encoding="utf-8") as out:
        rc = subprocess.run(argv, stdout=out, stderr=subprocess.STDOUT, env=dict(os.environ, REPRISE_MAC_MIN_DISK_GB="12"),
                            **NO_WINDOW).returncode
    text = Path(log).read_text(encoding="utf-8", errors="replace")
    passed = sum(int(n) for n in re.findall(r"^test result: ok\. (\d+) passed; 0 failed", text, re.M))
    failed = len(re.findall(r"^test result: FAILED", text, re.M))
    return (rc or (1 if failed else 0)), passed


def build_one(where, tag, dist, results):
    logs, key = dist / "logs", "rel_" + re.sub(r"[^a-z0-9]+", "_", tag.lower())
    common = ["--where", where, "--ref", tag, "--target-key", key]
    asset, test_log = dist / {"pc3": WIN, "mac": MAC, "linux": LINUX}[where], logs / ("%s-test.log" % where)
    if asset.is_file() and test_log.is_file():  # resume: this worker already passed and delivered
        text = test_log.read_text(encoding="utf-8", errors="replace")
        results[where] = {"build_rc": 0, "kept": True,
                          "tests_passed": sum(int(n) for n in re.findall(r"^test result: ok\. (\d+) passed; 0 failed", text, re.M))}
        return
    rc, passed = remote_build(logs / ("%s-test.log" % where), *common, "--", "cargo", "test", "--release")
    results[where] = {"test_rc": rc, "tests_passed": passed}
    if rc:
        return
    fetched = dist / ("fetch-" + where)
    if where == "mac":
        rel, args = "dist/" + MAC, ["--universal"]
    else:
        rel, args = "release/mouse-keys-record-replay" + (".exe" if where == "pc3" else ""), ["--", "cargo", "build", "--release"]
    rc, _ = remote_build(logs / ("%s-build.log" % where), *common, "--fetch", rel, "--to", str(fetched), *args)
    results[where]["build_rc"] = rc
    if rc == 0:
        shutil.copyfile(fetched / rel, asset)
        shutil.rmtree(fetched)
    if where == "mac":  # the Mac has little disk: drop this tool's src/target there, toolchain kept
        results[where]["clean_rc"] = remote_build(logs / "mac-clean.log", "--where", "mac", "--clean")[0]


def step_build(tag, dist):
    (dist / "logs").mkdir(parents=True, exist_ok=True)
    results = {}
    threads = [threading.Thread(target=build_one, args=(w, tag, dist, results)) for w in ("pc3", "mac", "linux")]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    for where in ("pc3", "mac", "linux"):
        say("build %s: %s" % (where, results.get(where)))
    ok = all(results.get(w, {}).get("build_rc") == 0 and results[w]["tests_passed"] > 0 for w in ("pc3", "mac", "linux"))
    for a in ASSETS:
        if (dist / a).is_file():
            say("asset %s %d bytes sha256 %s" % (a, (dist / a).stat().st_size, sha256(dist / a)))
    return ok


# ---------------------------------------------------------------- sign

def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def shipped_public_key(tag):
    """PUBLIC_KEY from src/update.rs at the tag, read the way release.yml's sign job reads it."""
    text = git("show", "%s:src/update.rs" % tag)
    i = text.index("PUBLIC_KEY: [u8; 32]")
    block = "\n".join(text[i:].splitlines()[:4])
    hexs = "".join(re.findall(r"0x([0-9a-f]{2})", block))
    if len(hexs) != 64:
        raise SystemExit("release-local: could not read PUBLIC_KEY from src/update.rs")
    return bytes.fromhex(hexs)


def step_sign(tag, dist):
    from cryptography.exceptions import InvalidSignature
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey

    want = shipped_public_key(tag)
    key = serialization.load_pem_private_key(KEY.read_bytes(), password=None)  # read in place, never copied
    if not isinstance(key, Ed25519PrivateKey):
        raise SystemExit("release-local: the signing key is not Ed25519")
    derived = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    if derived != want:
        raise SystemExit("release-local: signing key does not match PUBLIC_KEY in src/update.rs")
    say("signing key matches PUBLIC_KEY in src/update.rs at %s (%s)" % (tag, want.hex()))
    pub = Ed25519PublicKey.from_public_bytes(want)

    def sign(path):
        data = path.read_bytes()
        sig = key.sign(data)
        if len(sig) != 64:
            raise SystemExit("release-local: %s.sig is %d bytes, expected 64" % (path.name, len(sig)))
        Path(str(path) + ".sig").write_bytes(sig)
        try:
            pub.verify(Path(str(path) + ".sig").read_bytes(), path.read_bytes())
        except InvalidSignature:
            raise SystemExit("release-local: %s failed verification" % path.name)
        say("Signature Verified Successfully: %s" % path.name)

    for a in ASSETS:
        if not (dist / a).is_file() or not (dist / a).stat().st_size:
            raise SystemExit("release-local: missing asset " + a)
        sign(dist / a)
    names = list(ASSETS) + [a + ".sig" for a in ASSETS]
    sums = "".join("%s  %s\n" % (sha256(dist / n), n) for n in names)  # sha256sum text format
    (dist / "SHA256SUMS").write_bytes(sums.encode())
    sign(dist / "SHA256SUMS")
    for line in sums.splitlines():
        h, n = line.split("  ", 1)
        if sha256(dist / n) != h:
            raise SystemExit("release-local: SHA256SUMS mismatch for " + n)
        say("%s: OK" % n)
    if len(sums.splitlines()) != 6:
        raise SystemExit("release-local: SHA256SUMS should list 6 files")
    return True


# ---------------------------------------------------------------- publish

def gh(*args, check=True):
    return run("gh", *args, check=check)


def step_publish(tag, dist):
    files = [dist / n for n in list(ASSETS) + [a + ".sig" for a in ASSETS] + ["SHA256SUMS", "SHA256SUMS.sig"]]
    missing = [f.name for f in files if not f.is_file()]
    if missing:
        raise SystemExit("release-local: not signed yet, missing " + ", ".join(missing))
    notes = git("for-each-ref", "refs/tags/" + tag, "--format=%(contents)")
    notes = notes.split("-----BEGIN PGP SIGNATURE-----")[0].strip() or tag
    work = Path(tempfile.mkdtemp(prefix="mkrr-publish-"))
    try:
        (work / "notes.md").write_text(notes + "\n", encoding="utf-8", newline="\n")
        snap, pub = work / "snapshot", work / "public"
        archive = subprocess.run(["git", "-c", "core.autocrlf=false", "-C", str(REPO), "archive", "--format=tar", tag], capture_output=True, **NO_WINDOW)
        if archive.returncode:
            raise SystemExit("release-local: git archive failed")
        tar = work / "snap.tar"
        tar.write_bytes(archive.stdout)
        with tarfile.open(tar) as t:
            if any(m.issym() or m.islnk() or (m.isfile() and m.mode & 0o111) for m in t.getmembers()):
                raise SystemExit("release-local: the snapshot has links or executable files; publish from Linux instead")
            t.extractall(snap, filter="data")
        build_yml = (snap / ".github" / "workflows" / "build.yml").read_bytes()
        shutil.rmtree(snap / ".github")
        (snap / ".github" / "workflows").mkdir(parents=True)
        (snap / ".github" / "workflows" / "build.yml").write_bytes(build_yml)
        env = dict(os.environ, **IDENTITY)
        helper = ["-c", "credential.helper=", "-c", "credential.helper=!gh auth git-credential", "-c", "core.autocrlf=false"]
        run("git", *helper, "clone", "-q", "https://github.com/%s.git" % PUBLIC_REPO, str(pub))
        g = lambda *a, **kw: run("git", *helper, "-C", str(pub), *a, env=env, **kw)
        g("checkout", "-q", "-B", "main")
        g("config", "core.autocrlf", "false")
        g("rm", "-r", "-q", "--cached", "--ignore-unmatch", ".")
        for p in pub.iterdir():
            if p.name != ".git":
                shutil.rmtree(p) if p.is_dir() else p.unlink()
        shutil.copytree(snap, pub, dirs_exist_ok=True)
        g("add", "-A")
        if g("diff", "--cached", "--quiet", check=False).returncode == 0:
            say("public main already matches " + tag)
        else:
            g("commit", "-q", "-m", "Source for " + tag)
        g("push", "-q", "origin", "main")
        sha = g("rev-parse", "HEAD").stdout.strip()
        who = g("log", "-1", "--format=%an <%ae> / %cn <%ce>").stdout.strip()
        say("public main %s (%s)" % (sha, who))
        if gh("release", "view", tag, "--repo", PUBLIC_REPO, check=False).returncode == 0:
            gh("release", "upload", tag, "--repo", PUBLIC_REPO, "--clobber", *map(str, files))
        else:
            gh("release", "create", tag, "--repo", PUBLIC_REPO, "--target", sha, "--title", tag,
               "--notes-file", str(work / "notes.md"), "--latest", *map(str, files))
        tagged = gh("api", "repos/%s/commits/%s" % (PUBLIC_REPO, tag), "--jq", ".sha").stdout.strip()
        say("public %s -> %s, snapshot %s" % (tag, tagged, sha))
        if tagged != sha:
            raise SystemExit("release-local: public tag does not point at this release's snapshot")
        say("release https://github.com/%s/releases/tag/%s" % (PUBLIC_REPO, tag))
        return True
    finally:
        shutil.rmtree(work, ignore_errors=True)


# ---------------------------------------------------------------- check

def worker(where, line, stdin=None, timeout=900):
    sys.path.insert(0, os.environ.get("REPRISE_OFFLOAD_DIR", r"D:\sv-wt\r6-winworker\tools\offload"))
    import dispatch
    import remote_test2 as rt2
    if where == "mac":
        argv = dispatch.mac_argv(line)
    else:
        dispatch.WIN_PORT = rt2.TARGETS[where]["port"]
        argv = dispatch.win_remote_argv(line, None)
    got = subprocess.run(argv, input=stdin, capture_output=True, timeout=timeout, **NO_WINDOW)
    return got.returncode, (got.stdout + got.stderr).decode(errors="replace").replace("\r", "")


def step_check(tag, dist):
    logs = dist / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    sh_script = git("show", "%s:install.sh" % tag).encode()
    ps_script = git("show", "%s:install.ps1" % tag).encode()
    ok = True

    def record(name, rc, out, *must):
        nonlocal ok
        (logs / ("check-%s.log" % name)).write_text(out, encoding="utf-8")
        good = rc == 0 and all(re.search(m, out, re.M) for m in must)
        ok &= good
        say("check %s: rc %d, %s\n%s" % (name, rc, "PASS" if good else "FAIL", out.strip()))

    mac_app = ('app="/Applications/%s"; [ -d "$app" ] || app="$HOME/Applications/%s"; echo "installed: $app"; '
               'plutil -p "$app/Contents/Info.plist" | grep -E "CFBundleIdentifier|CFBundleShortVersionString|LSUIElement"; '
               'lipo -info "$app/Contents/MacOS/mouse-keys-record-replay"; codesign --verify --verbose "$app"') % (APP, APP)
    rc, out = worker("mac", 'set -e; %s version; MKRR_NO_LAUNCH=1 MKRR_OPENSSL=%s sh -s; %s' % (MAC_OPENSSL3, MAC_OPENSSL3, mac_app), sh_script)
    record("mac-openssl3", rc, out, r"^Signature OK$", r"CFBundleShortVersionString.*%s" % re.escape(tag.lstrip("v")),
           r"x86_64 arm64|arm64 x86_64", r"valid on disk")
    rc, out = worker("mac", 'set -e; export PATH=/usr/bin:/bin:/usr/sbin:/sbin MKRR_NO_LAUNCH=1 MKRR_OPENSSL=/usr/bin/openssl; '
                     'command -v openssl; openssl version; sh -s; codesign --verify --verbose "/Applications/%s" 2>&1 || '
                     'codesign --verify --verbose "$HOME/Applications/%s"' % (APP, APP), sh_script)
    record("mac-libressl", rc, out, r"^Checksum verified \(SHA256SUMS\)", r"valid on disk")
    rc, out = worker("win2080", 'wsl -d Ubuntu-22.04 -u build -e sh -c "openssl version; MKRR_NO_LAUNCH=1 sh -s && '
                     'ls -l $HOME/.local/bin/mouse-keys-record-replay && file $HOME/.local/bin/mouse-keys-record-replay"', sh_script)
    record("linux", rc, out, r"^Signature OK$", r"ELF 64-bit")
    # install.ps1 runs from a file under this tool's folder on pc3, as `Get-Content -Raw | Invoke-Expression` does in CI
    ps1 = WIN_TMP + r"\install-%s.ps1" % tag
    put = worker("pc3", 'powershell -NoProfile -NonInteractive -Command "$input | Set-Content -Encoding UTF8 -Path %s"' % ps1, ps_script)
    if put[0]:
        record("windows", put[0], put[1])
        return False
    rc, out = worker("pc3", 'set MKRR_NO_LAUNCH=1&& powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command '
                     '"$ErrorActionPreference=\'Stop\'; $PSVersionTable.PSVersion.ToString(); Get-Content -Raw %s | Invoke-Expression; '
                     'Get-Item $env:LOCALAPPDATA\\Programs\\MouseKeysRecordReplay\\mouse-keys-record-replay.exe | Format-List FullName, Length"' % ps1)
    record("windows", rc, out, r"^5\.1\.", r"^Signature OK", r"mouse-keys-record-replay\.exe")
    worker("pc3", 'del "%s" 2>nul' % ps1)
    return ok


def main(argv=None):
    ap = argparse.ArgumentParser(prog="release-local.py", description=__doc__.split("\n\n")[0])
    ap.add_argument("tag", help="the pushed v* tag, for example v0.1.4")
    ap.add_argument("--steps", default="build,sign,publish,check", help="comma list of build,sign,publish,check")
    ap.add_argument("--dist", help="folder for assets and logs (default dist-<tag> next to the repo)")
    a = ap.parse_args(argv)
    sys.stdout.reconfigure(errors="replace")
    if not re.fullmatch(r"v\d+\.\d+\.\d+", a.tag):
        raise SystemExit("release-local: tag must look like v1.2.3")
    git("rev-parse", "--verify", "refs/tags/%s^{commit}" % a.tag)
    dist = Path(a.dist) if a.dist else REPO.parent / ("mkrr-dist-" + a.tag)
    dist.mkdir(parents=True, exist_ok=True)
    steps = {"build": step_build, "sign": step_sign, "publish": step_publish, "check": step_check}
    for name in a.steps.split(","):
        if name not in steps:
            raise SystemExit("release-local: unknown step " + name)
        say("step %s" % name)
        if not steps[name](a.tag, dist):
            say("step %s FAILED, stopping" % name)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
