#!/bin/sh
set -eu

cd "$(dirname "$0")"
data_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}
case "$data_dir" in
    /*) ;;
    *) data_dir="$HOME/.local/share" ;;
esac
app_id=io.github.yashu7.opon
binary="$data_dir/opon/bin/opon"
applications="$data_dir/applications"

install -Dm755 opon "$binary"
install -Dm644 opon.png "$data_dir/icons/hicolor/256x256/apps/$app_id.png"
mkdir -p "$applications"

# The desktop entry and its Exec command each interpret backslash escapes.
escaped_binary=$(printf '%s' "$binary" | sed -e 's/\\/\\\\\\\\/g' -e 's/["$`]/\\\\&/g' -e 's/%/%%/g')
printf '[Desktop Entry]\nType=Application\nName=OPON\nExec="%s"\nIcon=%s\nCategories=Office;Utility;\nTerminal=false\n' \
    "$escaped_binary" "$app_id" > "$applications/$app_id.desktop"

printf 'Installed OPON in your application menu.\n'
