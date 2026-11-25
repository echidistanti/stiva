#!/usr/bin/env bash
# Packs a release build of Stiva as an AppImage: stiva-<version>-linux-x86_64.AppImage.
#
#   packaging/linux/build-appimage.sh <version> [binary] [output folder]
#
# The binary is target/release/stiva and the output folder is dist/ unless said otherwise. Nothing is bundled
# but the program: it needs no libraries of its own (the window, the graphics and D-Bus are found on the system
# when it starts), and ffmpeg is not shipped, Stiva asks for it on the first run.
#
# appimagetool is taken from $APPIMAGETOOL when that is set, and downloaded otherwise.
set -euo pipefail

version=${1:?usage: build-appimage.sh <version> [binary] [output folder]}
binary=${2:-target/release/stiva}
out=${3:-dist}

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
appdir=$work/Stiva.AppDir

# The folders as a Linux desktop expects them, and the same files at the top for the AppImage itself.
install -Dm755 "$binary" "$appdir/usr/bin/stiva"
install -Dm644 "$root/assets/icon-256.png" "$appdir/usr/share/icons/hicolor/256x256/apps/stiva.png"
sed "s/^Type=Application$/Type=Application\nX-AppImage-Version=$version/" "$here/stiva.desktop" >"$work/stiva.desktop"
install -Dm644 "$work/stiva.desktop" "$appdir/usr/share/applications/stiva.desktop"
install -m644 "$work/stiva.desktop" "$appdir/stiva.desktop"
install -m644 "$root/assets/icon-256.png" "$appdir/stiva.png"
ln -s stiva.png "$appdir/.DirIcon"
ln -s usr/bin/stiva "$appdir/AppRun"

tool=${APPIMAGETOOL:-}
if [ -z "$tool" ]; then
  tool=$work/appimagetool
  curl -fsSL --retry 3 -o "$tool" \
    https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi

# Runs appimagetool without FUSE, which a build machine or a container may not have.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$appdir" "$out/stiva-$version-linux-x86_64.AppImage"
echo "$out/stiva-$version-linux-x86_64.AppImage"
