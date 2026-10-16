#!/bin/sh
# Managed by Litematica-Nova shell integration v1
# Remove only this application's per-user registrations; never change another
# application's default association or remove user projections/cache data.
set -eu
data=${XDG_DATA_HOME:-"$HOME/.local/share"}
case "$data" in /*) ;; *) echo 'XDG_DATA_HOME must be absolute' >&2; exit 1;; esac
marker='# Managed by Litematica-Nova shell integration v1'
for relative in applications/com.litematica.nova.desktop mime/packages/com.litematica.nova.xml thumbnailers/com.litematica.nova.thumbnailer; do
    path=$data/$relative
    if [ -f "$path" ] && grep -Fq "$marker" "$path"; then rm -- "$path"; fi
done
icon=$data/icons/hicolor/64x64/apps/com.litematica.nova.png
if [ -f "$icon" ]; then rm -- "$icon"; fi
if command -v update-mime-database >/dev/null 2>&1; then update-mime-database "$data/mime"; fi
if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$data/applications"; fi
echo 'Litematica-Nova user shell registrations removed; user data retained.'
