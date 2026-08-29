#!/bin/bash
# Bump major/minor/patch, commit, tag, and push so GitHub Actions can cut a release.
# Usage: scripts/release.sh [major|minor|patch] [--no-push] [--dry-run]
set -euo pipefail

usage() {
    echo "usage: $0 [major|minor|patch] [--no-push] [--dry-run]" >&2
    exit 2
}

KIND=patch
PUSH=1
DRY=0
for arg in "$@"; do
    case $arg in
        major | minor | patch) KIND=$arg ;;
        --no-push) PUSH=0 ;;
        --dry-run) DRY=1 ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
cd "$ROOT"
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "not a git repository: $ROOT" >&2
    exit 1
fi

if [ "$DRY" -eq 0 ] && { ! git diff --quiet || ! git diff --cached --quiet; }; then
    echo "working tree is not clean" >&2
    exit 1
fi

CARGO=$ROOT/Cargo.toml
LOCK=$ROOT/Cargo.lock
ISS=$ROOT/packaging/windows/tunnel-ui.iss
OLD=$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"/\1/p' "$CARGO" | head -n1)
if [ -z "$OLD" ]; then
    echo "could not read version from Cargo.toml" >&2
    exit 1
fi

IFS=. read -r major minor patch <<EOF
$OLD
EOF
case $KIND in
    major)
        major=$((major + 1))
        minor=0
        patch=0
        ;;
    minor)
        minor=$((minor + 1))
        patch=0
        ;;
    patch) patch=$((patch + 1)) ;;
esac
NEW="${major}.${minor}.${patch}"
TAG="v${NEW}"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    echo "tag $TAG already exists locally" >&2
    exit 1
fi

echo "$OLD -> $NEW ($KIND)  tag $TAG"

if [ "$DRY" -eq 1 ]; then
    exit 0
fi

# Only the package version line (start of line), not crate dependency versions.
tmp=$(mktemp)
sed "s/^version = \"$OLD\"/version = \"$NEW\"/" "$CARGO" >"$tmp"
mv "$tmp" "$CARGO"

if [ -f "$LOCK" ]; then
    tmp=$(mktemp)
    awk -v old="$OLD" -v new="$NEW" '
        $0 == "name = \"tunnel-ui\"" { pkg = 1 }
        pkg && $0 ~ /^version = "/ {
            sub("\"" old "\"", "\"" new "\"")
            pkg = 0
        }
        { print }
    ' "$LOCK" >"$tmp"
    mv "$tmp" "$LOCK"
fi

if [ -f "$ISS" ]; then
    tmp=$(mktemp)
    sed "s/^  #define MyAppVersion \"$OLD\"/  #define MyAppVersion \"$NEW\"/" "$ISS" >"$tmp"
    mv "$tmp" "$ISS"
fi

git add -- "$CARGO"
[ -f "$LOCK" ] && git add -- "$LOCK"
[ -f "$ISS" ] && git add -- "$ISS"
git commit -m "$(cat <<EOF
Release $TAG

EOF
)"
git tag -a "$TAG" -m "Release $TAG"

if [ "$PUSH" -eq 1 ]; then
    git push origin HEAD
    git push origin "$TAG"
    echo "pushed HEAD and $TAG"
else
    echo "tagged $TAG locally; push with: git push origin HEAD && git push origin $TAG"
fi
