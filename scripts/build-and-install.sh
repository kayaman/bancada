#!/usr/bin/env bash
# Build a Bancada bundle for this machine and install it, per docs/INSTALL.md
# sections 1-2. Not the release ritual (see conventions.md §5) — this is for
# trying a local checkout on the machine you built it on.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  cat <<'EOF'
Usage: scripts/build-and-install.sh [--build-only] [--bundle rpm|deb|appimage]

  --build-only       Build the bundle but skip the install step.
  --bundle <target>  Force a bundle type instead of auto-detecting the
                      distro (or set BANCADA_BUNDLE in the environment).
EOF
}

BUNDLE="${BANCADA_BUNDLE:-}"
BUILD_ONLY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --build-only) BUILD_ONLY=1; shift ;;
    --bundle) BUNDLE="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

detect_bundle() {
  [ -f /etc/os-release ] || { echo appimage; return; }
  # shellcheck disable=SC1091
  . /etc/os-release
  case "${ID:-}" in
    fedora|rhel|centos|rocky|almalinux|opensuse*|suse) echo rpm; return ;;
    debian|ubuntu|linuxmint|pop) echo deb; return ;;
  esac
  case "${ID_LIKE:-}" in
    *fedora*|*rhel*|*suse*) echo rpm; return ;;
    *debian*|*ubuntu*) echo deb; return ;;
  esac
  echo appimage
}

[ -n "$BUNDLE" ] || BUNDLE="$(detect_bundle)"

case "$BUNDLE" in
  rpm|deb|appimage) ;;
  *) echo "Unsupported bundle: $BUNDLE (expected rpm, deb, or appimage)" >&2; exit 1 ;;
esac

VERSION="$(node -p "require('./package.json').version")"

echo "==> npm install"
npm install

echo "==> Building $BUNDLE bundle (version $VERSION)"
if [ "$BUNDLE" = appimage ]; then
  # Both hit stock Fedora (docs/INSTALL.md section 1): linuxdeploy needs FUSE
  # to run and its bundled binutils predates RELR relocations.
  export APPIMAGE_EXTRACT_AND_RUN=1
  export NO_STRIP=true
fi
npm run tauri -- build --bundles "$BUNDLE"

BUNDLE_DIR="target/release/bundle/$BUNDLE"

case "$BUNDLE" in
  rpm)
    PKG="$(find "$BUNDLE_DIR" -name '*.rpm' -print -quit)"
    ;;
  deb)
    PKG="$(find "$BUNDLE_DIR" -name '*.deb' -print -quit)"
    ;;
  appimage)
    PKG="$(find "$BUNDLE_DIR" -name '*.AppImage' -print -quit)"
    ;;
esac

if [ -z "$PKG" ]; then
  echo "Build finished but no $BUNDLE package was found under $BUNDLE_DIR" >&2
  exit 1
fi

echo "==> Built $PKG"

if [ "$BUILD_ONLY" = 1 ]; then
  echo "==> --build-only set, skipping install"
  exit 0
fi

case "$BUNDLE" in
  rpm)
    echo "==> Installing $PKG"
    if command -v dnf >/dev/null 2>&1; then
      sudo dnf install -y "$PKG"
    else
      sudo zypper install -y "$PKG"
    fi
    echo "==> Installed. Launch with: bancada"
    ;;
  deb)
    echo "==> Installing $PKG"
    sudo apt install -y "$PKG"
    echo "==> Installed. Launch with: bancada"
    ;;
  appimage)
    chmod +x "$PKG"
    echo "==> AppImage is self-contained: $PKG"
    echo "    Run it directly, or integrate it with Gear Lever / AppImageLauncher for a menu entry."
    ;;
esac
