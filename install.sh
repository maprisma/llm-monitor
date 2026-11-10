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
install -Dm755 target/release/maprisma-llm-monitor-tray "$bin/maprisma-llm-monitor-tray"
install -Dm644 scripts/llm-monitor.py "$data/maprisma-llm-monitor/scripts/llm-monitor.py"
install -Dm755 scripts/free-gpu.sh "$data/maprisma-llm-monitor/scripts/free-gpu.sh"
install -Dm644 "data/icons/$id-symbolic.svg" "$data/icons/hicolor/scalable/apps/$id-symbolic.svg"
# Desktop Exec has two escaping layers: quoted arguments, then string values.
desktop_arg() {
    local v=$1
    v=${v//\\/\\\\}
    v=${v//\"/\\\"}
    v=${v//\`/\\\`}
    v=${v//\$/\\\$}
    v=${v//\%/%%}
    v="\"$v\""
    v=${v//\\/\\\\}
    printf '%s' "$v"
}
app_exec=$(desktop_arg "$bin/maprisma-llm-monitor")
tray_exec=$(desktop_arg "$bin/maprisma-llm-monitor-tray")
mkdir -p "$data/applications" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
# Preserve all COSMIC applet metadata and its invocation without --window.
while IFS= read -r line; do
    if [[ $line == Exec=* ]]; then printf 'Exec=%s\n' "$app_exec"; else printf '%s\n' "$line"; fi
done < "data/$id.desktop" > "$data/applications/$id.desktop"
cat > "$data/applications/$id.Window.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Maprisma LLM Monitor
Name[de]=Maprisma LLM Monitor
Name[fr]=Maprisma LLM Monitor
Name[es]=Maprisma LLM Monitor
Name[it]=Maprisma LLM Monitor
Name[pt]=Maprisma LLM Monitor
Name[nl]=Maprisma LLM Monitor
Name[pl]=Maprisma LLM Monitor
Exec=$app_exec --window
Icon=$id-symbolic
Terminal=false
NoDisplay=false
Categories=System;Monitor;
EOF
cat > "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$id.Tray.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Maprisma LLM Monitor tray
Name[de]=Maprisma LLM Monitor im Systembereich
Name[fr]=Icône de notification Maprisma LLM Monitor
Name[es]=Bandeja de Maprisma LLM Monitor
Name[it]=Area di notifica di Maprisma LLM Monitor
Name[pt]=Bandeja do Maprisma LLM Monitor
Name[nl]=Maprisma LLM Monitor in het systeemvak
Name[pl]=Zasobnik Maprisma LLM Monitor
Exec=$tray_exec
Icon=$id-symbolic
Terminal=false
OnlyShowIn=KDE;
EOF
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Installed. Add “Maprisma LLM Monitor” in COSMIC Settings › Desktop › Panel › Applets."
