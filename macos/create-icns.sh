#!/bin/bash
# Usage: bash macos/create-icns.sh [output_dir]
# Requires: rsvg-convert (brew install librsvg), iconutil (Xcode CLI tools)
set -e

SVG="data/icons/org.gnome.Fractal.svg"
TMPDIR=$(mktemp -d)
ICONSET="$TMPDIR/fractal.iconset"
mkdir -p "$ICONSET"

for SIZE in 16 32 64 128 256 512 1024; do
    rsvg-convert -w "$SIZE" -h "$SIZE" "$SVG" -o "$ICONSET/icon_${SIZE}x${SIZE}.png"
done

cp "$ICONSET/icon_32x32.png"     "$ICONSET/icon_16x16@2x.png"
cp "$ICONSET/icon_64x64.png"     "$ICONSET/icon_32x32@2x.png"
cp "$ICONSET/icon_256x256.png"   "$ICONSET/icon_128x128@2x.png"
cp "$ICONSET/icon_512x512.png"   "$ICONSET/icon_256x256@2x.png"
cp "$ICONSET/icon_1024x1024.png" "$ICONSET/icon_512x512@2x.png"

iconutil -c icns "$ICONSET" -o "${1:-.}/fractal.icns"
rm -rf "$TMPDIR"
echo "Created ${1:-.}/fractal.icns"
