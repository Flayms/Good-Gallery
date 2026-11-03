#!/usr/bin/env bash
# Signs a release's installer for the app's updater (docs/specs/031-updates, REQ-031-09): writes <file>.sig, an Ed25519
# signature over the file's name and contents (domain::update::signed_message). Stops if the key in UPDATE_SIGNING_KEY
# isn't the one whose public half the app has built in, since no installed app could verify the update.
#
#   UPDATE_SIGNING_KEY=<PEM> ./packaging/sign-update.sh <file> [<public key file>]
set -euo pipefail

file=$1
public_key=${2:-"$(dirname "$0")/update-signing-key.txt"}
if [ -z "${UPDATE_SIGNING_KEY:-}" ]; then
  echo "UPDATE_SIGNING_KEY isn't set" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 700 "$work"
printf '%s\n' "$UPDATE_SIGNING_KEY" > "$work/key.pem"

# The last 32 bytes of the DER form are the raw public key.
actual=$(openssl pkey -in "$work/key.pem" -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n')
if [ "$actual" != "$(tr -d ' \n' < "$public_key")" ]; then
  echo "UPDATE_SIGNING_KEY doesn't match $public_key" >&2
  exit 1
fi

{ printf '%s\n' "$(basename "$file")"; cat "$file"; } > "$work/message"
openssl pkeyutl -sign -rawin -inkey "$work/key.pem" -in "$work/message" -out "$file.sig"
