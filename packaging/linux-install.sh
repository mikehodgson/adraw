#!/usr/bin/env bash
# Install this extracted ADraw for the current user: the adraw command, an
# application-menu entry, and "Open With ADraw" for .ans and .asc files.
# Run with --uninstall to remove them. Existing default applications for
# these file types are not changed.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin_dir="$HOME/.local/bin"
app_id=io.github.mikehodgson.adraw
binary="$bin_dir/adraw"
desktop="$data/applications/$app_id.desktop"
mime="$data/mime/packages/$app_id.xml"
icon="$data/icons/hicolor/256x256/apps/$app_id.png"

refresh() {
    if command -v update-mime-database > /dev/null; then
        update-mime-database "$data/mime"
    fi
    if command -v update-desktop-database > /dev/null; then
        update-desktop-database "$data/applications"
    fi
    if command -v gtk-update-icon-cache > /dev/null && [[ -f "$data/icons/hicolor/index.theme" ]]; then
        gtk-update-icon-cache --quiet "$data/icons/hicolor" || true
    fi
}

case "${1:-}" in
    "") ;;
    --uninstall)
        rm -f -- "$binary" "$desktop" "$mime" "$icon"
        mkdir -p "$data/mime/packages" "$data/applications"
        refresh
        echo "ADraw was removed from $bin_dir and your desktop menus. Your artwork was not changed."
        exit 0
        ;;
    *)
        echo "Usage: $0 [--uninstall]" >&2
        exit 2
        ;;
esac

if [[ ! -x "$here/adraw" || ! -f "$here/adraw.png" ]]; then
    echo "Run install.sh from the extracted ADraw folder; adraw or adraw.png is missing." >&2
    exit 1
fi
# Desktop-entry Exec values give special meaning to these characters.
if [[ "$binary" == *[\"\`\$\\%]* || "$binary" != /* ]]; then
    echo "Cannot register ADraw: $binary contains characters that desktop entries cannot launch." >&2
    exit 1
fi

mkdir -p "$bin_dir" "$(dirname "$desktop")" "$(dirname "$mime")" "$(dirname "$icon")"
install -m 755 "$here/adraw" "$binary.tmp"
mv -f -- "$binary.tmp" "$binary"
install -m 644 "$here/adraw.png" "$icon"
cat > "$mime" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="text/x-ansi">
    <comment>ANSI art</comment>
    <sub-class-of type="text/plain"/>
    <glob pattern="*.ans"/>
    <glob pattern="*.asc"/>
  </mime-type>
</mime-info>
EOF
cat > "$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=ADraw
GenericName=ANSI Art Editor
Comment=Draw and edit ANSI art
Exec="$binary" -- %f
Icon=$app_id
Terminal=false
Categories=Graphics;2DGraphics;RasterGraphics;
MimeType=text/x-ansi;
EOF
refresh
echo "ADraw is installed as $binary, with \"Open With ADraw\" for .ans and .asc files."
if [[ ":$PATH:" != *":$bin_dir:"* ]]; then
    echo "Add $bin_dir to PATH to run adraw from a terminal."
fi
