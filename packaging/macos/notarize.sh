#!/bin/sh
# Re-signs the bundle made by bundle.sh with the Developer ID certificate,
# notarizes it, staples the ticket and rewrites the release zip from the
# stapled bundle, so the .sig and SHA256SUMS made later cover the final file.
#
# Usage: notarize.sh <out-dir>   (the same out-dir given to bundle.sh)
# Env:   MACOS_DEVID_P12 (base64 .p12), MACOS_DEVID_P12_PASSWORD,
#        ASC_KEY_P8 (base64 .p8), ASC_KEY_ID, ASC_ISSUER_ID
# Without MACOS_DEVID_P12 it does nothing and the ad-hoc zip ships as is.
# Set NOTARIZE_DRY_RUN=1 to check inputs and print the plan without signing.
set -eu

out=$1
app="$out/Mouse Keys Record Replay.app"
zip="$out/mouse-keys-record-replay-macos.zip"
tmp=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mkrr-notarize.$$

if [ -z "${MACOS_DEVID_P12:-}" ]; then
  echo "MACOS_DEVID_P12 is not set: shipping the ad-hoc signed build"
  exit 0
fi
for v in MACOS_DEVID_P12 MACOS_DEVID_P12_PASSWORD ASC_KEY_P8 ASC_KEY_ID ASC_ISSUER_ID; do
  eval "val=\${$v:-}"
  [ -n "$val" ] || { echo "$v is not set"; exit 1; }
done
[ -d "$app" ] || { echo "missing $app (run bundle.sh first)"; exit 1; }

if [ "${NOTARIZE_DRY_RUN:-}" = 1 ]; then
  echo "dry run: would sign, notarize and staple $app, then rewrite $zip"
  exit 0
fi

keychain="$tmp/signing.keychain-db"
kc_pass=$(openssl rand -hex 24)
mkdir -p "$tmp"
chmod 700 "$tmp"
cleanup() {
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT

printf '%s' "$MACOS_DEVID_P12" | base64 --decode > "$tmp/devid.p12"
printf '%s' "$ASC_KEY_P8" | base64 --decode > "$tmp/AuthKey.p8"

security create-keychain -p "$kc_pass" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$kc_pass" "$keychain"
security import "$tmp/devid.p12" -k "$keychain" -P "$MACOS_DEVID_P12_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kc_pass" "$keychain" >/dev/null
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

identity=$(security find-identity -v -p codesigning "$keychain" | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -n 1)
[ -n "$identity" ] || { echo "no Developer ID Application identity in the .p12"; exit 1; }
echo "signing as: $identity"

# The app needs no entitlements: Accessibility and Input Monitoring are TCC
# grants, and nothing in it generates code at runtime.
codesign --force --options runtime --timestamp --sign "$identity" --keychain "$keychain" \
  "$app/Contents/MacOS/mouse-keys-record-replay"
codesign --force --options runtime --timestamp --sign "$identity" --keychain "$keychain" "$app"
codesign --verify --strict --deep --verbose=2 "$app"
codesign -dv --verbose=2 "$app" 2>&1 | grep -E '^(Authority|TeamIdentifier|Identifier|flags)='

ditto -c -k --keepParent "$app" "$tmp/submit.zip"
xcrun notarytool submit "$tmp/submit.zip" \
  --key "$tmp/AuthKey.p8" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID" \
  --wait --timeout 30m --output-format json > "$tmp/submit.json"
cat "$tmp/submit.json"
status=$(sed -n 's/.*"status" *: *"\([^"]*\)".*/\1/p' "$tmp/submit.json")
if [ "$status" != Accepted ]; then
  id=$(sed -n 's/.*"id" *: *"\([^"]*\)".*/\1/p' "$tmp/submit.json")
  if [ -n "$id" ]; then
    xcrun notarytool log "$id" \
      --key "$tmp/AuthKey.p8" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID" || true
  fi
  echo "notarization status: ${status:-unknown}"
  exit 1
fi

xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl -a -vvv -t exec "$app" 2>&1 | tee "$tmp/spctl.txt"
grep -q "source=Notarized Developer ID" "$tmp/spctl.txt" || { echo "Gatekeeper does not report Notarized Developer ID"; exit 1; }

rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "notarized and stapled: $zip"
