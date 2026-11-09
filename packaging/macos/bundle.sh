#!/bin/sh
# Wraps a built binary into the .app bundle and zips it the way the updater
# expects (ditto, parent folder kept). Usage: bundle.sh <binary> <out-dir>
set -eu
bin=$1
out=$2
here=$(cd "$(dirname "$0")" && pwd)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/../../Cargo.toml" | head -n 1)
app="$out/Mouse Keys Record Replay.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
cp "$bin" "$app/Contents/MacOS/mouse-keys-record-replay"
chmod 755 "$app/Contents/MacOS/mouse-keys-record-replay"
cp "$here/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
codesign --force --deep --sign - "$app"
rm -f "$out/mouse-keys-record-replay-macos.zip"
ditto -c -k --keepParent "$app" "$out/mouse-keys-record-replay-macos.zip"
