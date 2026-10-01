#!/usr/bin/env bash
# Cut a release: bump the workspace version, commit, tag vX.Y.Z and push.
# The Release workflow then builds the binaries, publishes the GitHub
# Release and updates the Homebrew formula.
#
# Usage: scripts/release.sh X.Y.Z        (or: major | minor | patch)
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

current=$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -1)
IFS=. read -r maj min pat <<<"$current"
case "${1:-}" in
  major) next="$((maj + 1)).0.0" ;;
  minor) next="$maj.$((min + 1)).0" ;;
  patch) next="$maj.$min.$((pat + 1))" ;;
  "") echo "usage: scripts/release.sh X.Y.Z | major | minor | patch   (current: $current)" >&2; exit 1 ;;
  *) next="$1" ;;
esac
[[ $next =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a version: $next" >&2; exit 1; }
[ "$next" != "$current" ] || { echo "already at $current" >&2; exit 1; }

branch=$(git rev-parse --abbrev-ref HEAD)
[ "$branch" = main ] || { echo "release from main (you are on $branch)" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "commit or stash your changes first" >&2; git status --short; exit 1; }
git fetch origin --tags --quiet
git merge --ff-only origin/main --quiet
! git rev-parse -q --verify "refs/tags/v$next" >/dev/null || { echo "tag v$next already exists" >&2; exit 1; }

echo "schema $current → $next"
# the workspace version lives in Cargo.toml; every crate inherits it
# (perl: the first matching line only, and the same on macOS and Linux)
CUR="$current" NEXT="$next" perl -pi -e 'if (!$done && s/^version = "\Q$ENV{CUR}\E"/version = "$ENV{NEXT}"/) { $done = 1 }' Cargo.toml
grep -q "^version = \"$next\"" Cargo.toml || { echo "failed to bump the version in Cargo.toml" >&2; exit 1; }
cargo update --workspace --quiet            # lockfile entries for the workspace crates
SCHEMA_SKIP_WASM=1 cargo build --quiet -p schema 2>/dev/null || cargo build --quiet -p schema
cargo test --workspace --quiet

git add Cargo.toml Cargo.lock
git commit -q -m "Release v$next"
git tag -a "v$next" -m "schema v$next"
git push origin main "v$next"

echo
echo "tagged v$next — the Release workflow is building:"
echo "  $(git remote get-url origin | sed -E 's#^git@github.com:#https://github.com/#; s#\.git$##')/actions"
