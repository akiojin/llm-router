#!/usr/bin/env bash
# Update Cargo workspace versions to the semantic-release version.
# Expects cargo-edit (cargo-set-version) to be available.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <version>" >&2
  exit 1
fi

VERSION="$1"

if ! command -v cargo-set-version >/dev/null 2>&1; then
  echo "cargo-set-version not found; installing cargo-edit..." >&2
  cargo install cargo-edit --locked
fi

echo "Setting Cargo workspace version to ${VERSION}"
cargo set-version --workspace "${VERSION}"

echo "Workspace version updated."
