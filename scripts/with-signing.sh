#!/bin/sh
# Run a command (a macOS app build) with NS2Bench's stable signing identity available.
#
# Why: an ad-hoc signed app is identified by a hash that changes on every build and
# update, so macOS forgets its Camera/Microphone permission each time. A fixed
# (self-signed) certificate keeps the same identity across builds and updates.
#
# The certificate comes from $NS2BENCH_SIGNING_P12 (base64) + $NS2BENCH_SIGNING_PASSWORD
# (CI secrets), or from ~/.ns2bench/signing/ns2bench.p12 + ~/.ns2bench/signing/password.
# Without one the command runs unchanged (ad-hoc signing). The certificate is imported
# into a throwaway keychain that is removed from the search list and deleted afterwards.
set -eu
if [ "$(uname)" != Darwin ]; then exec "$@"; fi
dir=$(mktemp -d "${TMPDIR:-/tmp}/ns2bench-sign.XXXXXX")
p12="$dir/id.p12"
if [ -n "${NS2BENCH_SIGNING_P12:-}" ]; then
  printf '%s' "$NS2BENCH_SIGNING_P12" | base64 -d > "$p12"
  pass="${NS2BENCH_SIGNING_PASSWORD:?NS2BENCH_SIGNING_PASSWORD is not set}"
elif [ -f "$HOME/.ns2bench/signing/ns2bench.p12" ]; then
  cp "$HOME/.ns2bench/signing/ns2bench.p12" "$p12"
  pass=$(cat "$HOME/.ns2bench/signing/password")
else
  rm -rf "$dir"
  echo "with-signing: no signing certificate; the app will be signed ad-hoc" >&2
  exec "$@"
fi
kc="$dir/ns2bench.keychain-db"
kcpass=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
# One keychain path per line (paths may contain spaces): split on newlines only.
old=$(security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"[[:space:]]*$//')
nl='
'
cleanup() {
  # shellcheck disable=SC2086
  IFS=$nl; security list-keychains -d user -s $old >/dev/null 2>&1 || true; unset IFS
  security delete-keychain "$kc" >/dev/null 2>&1 || true
  rm -rf "$dir"
}
trap cleanup EXIT INT TERM
security create-keychain -p "$kcpass" "$kc"
security set-keychain-settings -t 3600 -u "$kc"
security unlock-keychain -p "$kcpass" "$kc"
security import "$p12" -k "$kc" -P "$pass" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple: -k "$kcpass" "$kc" >/dev/null
# shellcheck disable=SC2086
IFS=$nl; security list-keychains -d user -s "$kc" $old; unset IFS
id=$(security find-identity -p codesigning "$kc" | awk '/^ *1\)/ {print $2; exit}')
[ -n "$id" ] || { echo "with-signing: no code-signing identity in the certificate" >&2; exit 1; }
echo "with-signing: signing with certificate $id" >&2
APPLE_SIGNING_IDENTITY="$id" "$@"
