#!/usr/bin/env bash
# Builds the Linux AppImage inside Ubuntu 22.04 (glibc 2.35), so it also runs on older distros
# (a build made directly on a very recent distro needs that distro's glibc and won't start elsewhere).
# Run it from the project folder:   bash build-appimage-old-glibc.sh
# Needs Docker (or Podman: DOCKER=podman bash build-appimage-old-glibc.sh) and an internet connection.
# The AppImage appears in ./dist/release/. The first run takes a long time; later runs reuse the cache.
set -euo pipefail
cd "$(dirname "$0")"
DOCKER="${DOCKER:-docker}"
command -v "$DOCKER" >/dev/null || { echo "error: '$DOCKER' not found. Install Docker (or set DOCKER=podman)." >&2; exit 1; }
mkdir -p dist

"$DOCKER" run --rm -t \
  -v "$PWD":/src \
  -v filmcraft-cargo:/root/.cargo \
  -v filmcraft-rustup:/root/.rustup \
  -v filmcraft-target:/target \
  -e CARGO_TARGET_DIR=/target \
  -e GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-leozan14114/filmcraft}" \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  -w /src ubuntu:22.04 bash -euxc '
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends build-essential pkg-config libasound2-dev curl ca-certificates git file zsync desktop-file-utils
    git config --global --add safe.directory /src
    if [ ! -x /root/.cargo/bin/rustup ]; then
      curl --proto "=https" --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.95
    fi
    export PATH=/root/.cargo/bin:$PATH
    rustup toolchain install 1.95 --profile minimal
    ./packaging/linux/package.sh --formats appimage
    chown -R "$HOST_UID:$HOST_GID" /src/dist
    ldd --version | head -1
  '

echo
echo "Done. Look in dist/release/ for the AppImage."
