#!/usr/bin/env bash
# Build and install Maprisma LLM Monitor for the current user (no root needed):
#   ~/.local/bin/maprisma-llm-monitor                  the applet
#   ~/.local/share/maprisma-llm-monitor/scripts/       the collector and free-gpu.sh
#   ~/.local/share/applications/…desktop             makes it an applet for the panel
#   ~/.local/share/icons/hicolor/scalable/apps/…svg  the chip icon
# Then add it in COSMIC Settings › Desktop › Panel › Applets.
set -euo pipefail
cd "$(dirname "$0")"
id=io.github.ubruckhaus.CosmicMaprismaLlmMonitor
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin"

cargo build --release
install -Dm755 target/release/maprisma-llm-monitor "$bin/maprisma-llm-monitor"
install -Dm644 scripts/llm-monitor.py "$data/maprisma-llm-monitor/scripts/llm-monitor.py"
install -Dm755 scripts/free-gpu.sh "$data/maprisma-llm-monitor/scripts/free-gpu.sh"
install -Dm644 "data/icons/$id-symbolic.svg" "$data/icons/hicolor/scalable/apps/$id-symbolic.svg"
# The panel starts applets without ~/.local/bin in PATH: the full path.
sed "s|^Exec=.*|Exec=$bin/maprisma-llm-monitor|" "data/$id.desktop" > "$data/applications/$id.desktop"
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Installed. Add “Maprisma LLM Monitor” in COSMIC Settings › Desktop › Panel › Applets."
