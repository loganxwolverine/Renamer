#!/usr/bin/env bash
# Installation sur Fedora.
#  - depuis une release (binaire fourni à côté de ce script) : installe seulement les dépendances d'exécution
#  - depuis les sources : installe les outils de compilation puis compile
set -euo pipefail
cd "$(dirname "$0")"

if [ -x ./openhtpc-renamer ]; then
  echo "==> Dépendances d'exécution (sudo)"
  sudo dnf install -y gtk4 libadwaita
  BIN=./openhtpc-renamer
else
  echo "==> Dépendances de compilation (sudo)"
  sudo dnf install -y rust cargo gtk4-devel libadwaita-devel gcc pkgconf-pkg-config
  echo "==> Compilation"
  cargo build --release
  BIN=target/release/openhtpc-renamer
fi
command -v ffprobe >/dev/null || sudo dnf install -y ffmpeg-free

echo "==> Installation dans ~/.local"
install -Dm755 "$BIN" "$HOME/.local/bin/openhtpc-renamer"
install -Dm644 data/io.github.openhtpc.Renamer.desktop \
  "$HOME/.local/share/applications/io.github.openhtpc.Renamer.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

echo "Terminé. Lancez « OpenHTPC Renamer » depuis le menu, ou : openhtpc-renamer"
