#!/bin/sh
# Build a Slackware .txz from a prebuilt tunnel-ui binary.
# Usage: build-txz.sh <binary> <version> [outdir]
set -eu

if [ "$#" -lt 2 ]; then
    echo "usage: $0 <binary> <version> [outdir]" >&2
    exit 2
fi

BIN=$1
VERSION=$2
OUTDIR=${3:-.}
ARCH=${ARCH:-$(uname -m)}
BUILD=${BUILD:-1}

case $VERSION in
    v*) VERSION=${VERSION#v} ;;
esac

if [ ! -f "$BIN" ]; then
    echo "binary not found: $BIN" >&2
    exit 1
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DESC=$SCRIPT_DIR/slack-desc
if [ ! -f "$DESC" ]; then
    echo "missing $DESC" >&2
    exit 1
fi

PKGNAME="tunnel-ui-${VERSION}-${ARCH}-${BUILD}"
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT

mkdir -p "$ROOT/usr/local/bin" "$ROOT/usr/local/doc/tunnel-ui-$VERSION" "$ROOT/install"
install -m 755 "$BIN" "$ROOT/usr/local/bin/tunnel-ui"
if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded "$ROOT/usr/local/bin/tunnel-ui" 2>/dev/null || true
fi

REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
if [ -f "$REPO_ROOT/README.md" ]; then
    install -m 644 "$REPO_ROOT/README.md" "$ROOT/usr/local/doc/tunnel-ui-$VERSION/README"
fi
install -m 644 "$DESC" "$ROOT/install/slack-desc"

mkdir -p "$OUTDIR"
OUTDIR=$(CDPATH= cd -- "$OUTDIR" && pwd)
OUT="$OUTDIR/${PKGNAME}.txz"

# installpkg expects a tar of usr/ and install/ with root ownership.
( cd "$ROOT" && tar --numeric-owner --owner=0 --group=0 -cJf "$OUT" usr install )

echo "$OUT"
