#!/bin/sh
# Installs the latest Mouse Keys Record Replay release on macOS or Linux.
#   curl -fsSL https://mouserec.com/install.sh | sh
# The download is checked against the release signature before it is
# installed. Set MKRR_NO_LAUNCH=1 to install without starting the app.
set -eu

REPO="tiktool/mouse-keys-record-replay"
BASE="https://github.com/$REPO/releases/latest/download"
APP_NAME="Mouse Keys Record Replay"
PUBLIC_KEY_PEM="-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAuNFBmOjrwZCgQiQi+kemAO7kwutgcHyjkMdzTggxd4s=
-----END PUBLIC KEY-----"

say() { printf '%s\n' "$*"; }
fail() { printf 'Error: %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
  Darwin) os=macos; asset="mouse-keys-record-replay-macos.zip" ;;
  Linux)
    [ "$(uname -m)" = "x86_64" ] || fail "only x86_64 Linux builds are published."
    os=linux; asset="mouse-keys-record-replay-linux-x64" ;;
  *) fail "this installer supports macOS and Linux. On Windows run: irm https://mouserec.com/install.ps1 | iex" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required."

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading $asset"
curl -fsSL --proto '=https' -o "$tmp/$asset" "$BASE/$asset" || fail "download failed."
curl -fsSL --proto '=https' -o "$tmp/$asset.sig" "$BASE/$asset.sig" || fail "signature download failed."

printf '%s\n' "$PUBLIC_KEY_PEM" > "$tmp/release.pub"
[ "$(wc -c < "$tmp/$asset.sig" | tr -d ' ')" = "64" ] || fail "the signature file is malformed. Nothing was installed."
# Ed25519 checks need OpenSSL 3 (-rawin). The first openssl on PATH may be
# older, so Homebrew's openssl@3 and the system copy are tried as well.
verified=
for ssl in "$(command -v openssl 2>/dev/null)" /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/opt/openssl@3/bin/openssl /usr/bin/openssl; do
  [ -n "$ssl" ] && [ -x "$ssl" ] || continue
  if out=$("$ssl" pkeyutl -verify -pubin -inkey "$tmp/release.pub" -rawin -in "$tmp/$asset" -sigfile "$tmp/$asset.sig" 2>&1); then
    verified=1; break
  fi
  case "$out" in
    *"Signature Verification Failure"*|*"verification failure"*)
      fail "the download failed its signature check. Nothing was installed." ;;
  esac
done
[ -n "$verified" ] || fail "no openssl here can check Ed25519 signatures. Install OpenSSL 3 (for example: brew install openssl) and run this again."
say "Signature OK"

if [ "$os" = macos ]; then
  dest=/Applications
  if [ ! -w "$dest" ]; then
    dest="$HOME/Applications"
    mkdir -p "$dest"
  fi
  mkdir "$tmp/unpacked"
  ditto -x -k "$tmp/$asset" "$tmp/unpacked" || fail "could not unpack the app."
  [ -d "$tmp/unpacked/$APP_NAME.app" ] || fail "the download did not contain $APP_NAME.app."
  rm -rf "$dest/$APP_NAME.app"
  ditto "$tmp/unpacked/$APP_NAME.app" "$dest/$APP_NAME.app"
  xattr -dr com.apple.quarantine "$dest/$APP_NAME.app" 2>/dev/null || true
  say "Installed to $dest/$APP_NAME.app"
  if [ "${MKRR_NO_LAUNCH:-}" != "1" ]; then
    open "$dest/$APP_NAME.app"
  fi
else
  dest="$HOME/.local/bin"
  mkdir -p "$dest"
  target="$dest/mouse-keys-record-replay"
  # Rename first so a running copy keeps its file and the new one lands cleanly.
  [ -e "$target" ] && mv -f "$target" "$target.old"
  cp "$tmp/$asset" "$target"
  chmod +x "$target"
  rm -f "$target.old"
  say "Installed to $target"
  if [ "${MKRR_NO_LAUNCH:-}" != "1" ]; then
    nohup "$target" >/dev/null 2>&1 &
  fi
fi
say "Done. Press Ctrl+Alt+Shift+F1 to record (Cmd+Option+Shift+F1 on macOS)."
