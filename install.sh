#!/usr/bin/env bash
# Installation sur Fedora : dépendances, compilation, lanceur dans le menu.
set -euo pipefail
cd "$(dirname "$0")"

echo "==> Dépendances (sudo)"
sudo dnf install -y rust cargo gtk4-devel libadwaita-devel gcc pkgconf-pkg-config
command -v ffprobe >/dev/null || sudo dnf install -y ffmpeg-free

echo "==> Compilation"
cargo build --release

echo "==> Installation dans ~/.local"
install -Dm755 target/release/openhtpc-renamer "$HOME/.local/bin/openhtpc-renamer"
install -Dm644 data/io.github.openhtpc.Renamer.desktop \
  "$HOME/.local/share/applications/io.github.openhtpc.Renamer.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

echo "Terminé. Lancez « OpenHTPC Renamer » depuis le menu, ou : openhtpc-renamer"
