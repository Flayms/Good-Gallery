#!/usr/bin/env bash
# Creates the key that signs updates (docs/specs/031-updates, REQ-031-09), once: stores the private key as the repository
# secret UPDATE_SIGNING_KEY, where nobody can read it back, and writes the public key into the file the app builds in.
# Commit that file. Needs openssl and an authenticated gh.
#
# A new key later means installed apps refuse updates until people install a new version by hand.
set -euo pipefail

public_key="$(dirname "$0")/update-signing-key.txt"
if [ -s "$public_key" ]; then
  echo "$public_key already holds a key. Empty it first if you really mean to replace it." >&2
  exit 1
fi

key=$(openssl genpkey -algorithm ed25519)
printf '%s\n' "$key" | gh secret set UPDATE_SIGNING_KEY --repo Flayms/Good-Gallery
printf '%s\n' "$key" | openssl pkey -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n' > "$public_key"
echo "Stored the secret and wrote $public_key. Commit that file."
