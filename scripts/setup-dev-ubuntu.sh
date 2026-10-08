#!/usr/bin/env bash
# =============================================================================
#  SysPulse - Multipurpose System Utility for Ubuntu
#  File   : scripts/setup-dev-ubuntu.sh
#  Author : Praneeth
#  License: MIT
#  Purpose: Install every build dependency needed to develop SysPulse on
#           Ubuntu 22.04+ (system libs via apt, Rust via rustup, Node.js LTS
#           into ~/.local/opt/node). Safe to re-run.
# =============================================================================
set -euo pipefail

NODE_VERSION="v22.11.0"

echo "==> [1/3] System libraries (sudo apt)"
sudo apt-get update
sudo apt-get install -y \
  build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-pipewire \
  pciutils usbutils lshw policykit-1 libudev-dev \
  gstreamer1.0-x gstreamer1.0-pulseaudio gstreamer1.0-plugins-ugly gstreamer1.0-libav

echo "==> [2/3] Rust toolchain (rustup, user-local)"
if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
"$HOME/.cargo/bin/rustup" update stable

echo "==> [3/3] Node.js ${NODE_VERSION} (user-local, ~/.local/opt/node)"
if [ ! -x "$HOME/.local/opt/node/bin/node" ]; then
  mkdir -p "$HOME/.local/opt"
  tmp="$(mktemp -d)"
  curl -fsSL "https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.xz" -o "$tmp/node.tar.xz"
  tar -xJf "$tmp/node.tar.xz" -C "$HOME/.local/opt"
  rm -rf "$HOME/.local/opt/node"
  mv "$HOME/.local/opt/node-${NODE_VERSION}-linux-x64" "$HOME/.local/opt/node"
  rm -rf "$tmp"
fi
mkdir -p "$HOME/.local/bin"
for b in node npm npx; do ln -sf "$HOME/.local/opt/node/bin/$b" "$HOME/.local/bin/$b"; done

echo
echo "Done. Versions:"
"$HOME/.cargo/bin/cargo" --version
"$HOME/.local/bin/node" --version
echo "Open a new terminal (or 'source ~/.cargo/env') so cargo is on PATH."
