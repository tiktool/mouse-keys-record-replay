#!/usr/bin/env bash
# Turns the Developer ID Application certificate the owner downloads into the
# GitHub secrets release.yml uses to sign and notarize the macOS build.
#
# Inputs (all in ~/.keys unless overridden):
#   mkrr-devid.key                  private key the CSR was made from
#   developerID_application.cer     certificate downloaded from developer.apple.com
#   ~/.apple/AuthKey_<ASC_KEY_ID>.p8
# Output: mkrr-devid.p12 + mkrr-devid.p12.password in ~/.keys, and the secrets
#   MACOS_DEVID_P12, MACOS_DEVID_P12_PASSWORD, ASC_KEY_P8, ASC_KEY_ID,
#   ASC_ISSUER_ID on the private repo. No secret value is printed.
#
# Usage: bash tools/finish-devid.sh [--dry-run]
set -euo pipefail

keys=${MKRR_KEYS_DIR:-$HOME/.keys}
repo=${MKRR_REPO:-tiktool/mouse-keys-record-replay-source}
asc_key_id=${ASC_KEY_ID:-T535UXXPAD}
asc_p8=${ASC_KEY_P8_PATH:-$HOME/.apple/AuthKey_${asc_key_id}.p8}
asc_script=${ASC_SCRIPT:-/d/Works/remote-dslr/ios/scripts/asc.mjs}
key="$keys/mkrr-devid.key"
cer="$keys/developerID_application.cer"
p12="$keys/mkrr-devid.p12"
p12_pass_file="$keys/mkrr-devid.p12.password"
dry=${1:-}

for f in "$key" "$cer" "$asc_p8"; do
  [ -s "$f" ] || { echo "missing $f"; exit 1; }
done

if [ -n "${ASC_ISSUER_ID:-}" ]; then
  issuer=$ASC_ISSUER_ID
else
  issuer=$(sed -n "s/.*ASC_ISSUER_ID || '\([0-9a-f-]*\)'.*/\1/p" "$asc_script" | head -n 1)
fi
[[ $issuer =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] || { echo "could not read the ASC issuer id"; exit 1; }

umask 077
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Apple serves the certificate as DER; accept PEM too.
if ! openssl x509 -inform DER -in "$cer" -out "$work/cert.pem" 2>/dev/null; then
  openssl x509 -in "$cer" -out "$work/cert.pem"
fi
subject=$(openssl x509 -in "$work/cert.pem" -noout -subject)
echo "certificate $subject"
[[ $subject == *"Developer ID Application"* ]] || { echo "this is not a Developer ID Application certificate"; exit 1; }
openssl x509 -in "$work/cert.pem" -noout -checkend 0 >/dev/null || { echo "certificate has expired"; exit 1; }
a=$(openssl x509 -in "$work/cert.pem" -noout -pubkey | openssl sha256)
b=$(openssl pkey -in "$key" -pubout | openssl sha256)
[ "$a" = "$b" ] || { echo "certificate does not match $key"; exit 1; }

pass=$(openssl rand -hex 24)
# SHA1/3DES keeps the .p12 importable by the macOS security tool, which
# rejects the OpenSSL 3 default (AES/PBKDF2) on some releases.
printf '%s' "$pass" > "$work/pass"
MKRR_P12_PASS=$pass openssl pkcs12 -export -inkey "$key" -in "$work/cert.pem" \
  -name "Developer ID Application" \
  -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1 \
  -passout env:MKRR_P12_PASS -out "$work/devid.p12"
MKRR_P12_PASS=$pass openssl pkcs12 -in "$work/devid.p12" -passin env:MKRR_P12_PASS -noout
cp "$work/devid.p12" "$p12"
cp "$work/pass" "$p12_pass_file"
echo "wrote $p12 and $p12_pass_file"

if [ "$dry" = --dry-run ]; then
  echo "dry run: secrets not set on $repo"
  exit 0
fi

set_secret() {
  timeout 60 gh secret set "$1" --env release --repo "$repo" > /dev/null
  echo "set $1"
}
base64 -w0 "$p12" | set_secret MACOS_DEVID_P12
set_secret MACOS_DEVID_P12_PASSWORD < "$p12_pass_file"
base64 -w0 "$asc_p8" | set_secret ASC_KEY_P8
printf '%s' "$asc_key_id" | set_secret ASC_KEY_ID
printf '%s' "$issuer" | set_secret ASC_ISSUER_ID
timeout 60 gh secret list --env release --repo "$repo" | grep -E '^(MACOS_DEVID_P12|MACOS_DEVID_P12_PASSWORD|ASC_KEY_P8|ASC_KEY_ID|ASC_ISSUER_ID)\b'
