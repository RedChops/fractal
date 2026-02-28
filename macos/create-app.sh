#!/bin/bash
# Usage: bash macos/create-app.sh
# Run from the repo root after: meson compile -C builddir
#
# Prerequisites (brew):
#   brew install dylibbundler librsvg
#
# Produces Fractal.app in the current directory.
set -e

BREW="$(brew --prefix)"
APP="Fractal.app"

echo "==> Building release binary..."
cargo build --release

echo "==> Scaffolding $APP..."
rm -rf "$APP"
mkdir -p "$APP/Contents/"{MacOS,Frameworks}
mkdir -p "$APP/Contents/Resources/share/glib-2.0/schemas"
mkdir -p "$APP/Contents/Resources/share/locale"
mkdir -p "$APP/Contents/Resources/lib/gio/modules"
mkdir -p "$APP/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders"
mkdir -p "$APP/Contents/Resources/lib/gstreamer-1.0"

cp macos/Info.plist "$APP/Contents/"
cp target/release/fractal "$APP/Contents/MacOS/Fractal"

echo "==> Copying GResource data files..."
cp builddir/data/resources/resources.gresource "$APP/Contents/Resources/"
cp builddir/src/ui-resources.gresource         "$APP/Contents/Resources/"

echo "==> Copying and compiling GSettings schemas..."
# App schema
if [ -f builddir/data/org.gnome.Fractal.gschema.xml ]; then
    cp builddir/data/org.gnome.Fractal.gschema.xml \
       "$APP/Contents/Resources/share/glib-2.0/schemas/"
fi
# Homebrew schemas (GTK, Adwaita, etc.)
if ls "$BREW/share/glib-2.0/schemas/"*.xml &>/dev/null; then
    cp "$BREW/share/glib-2.0/schemas/"*.xml \
       "$APP/Contents/Resources/share/glib-2.0/schemas/" 2>/dev/null || true
fi
glib-compile-schemas "$APP/Contents/Resources/share/glib-2.0/schemas/"

echo "==> Copying GLib IO modules (TLS)..."
if ls "$BREW/lib/gio/modules/"libgio*.so &>/dev/null 2>&1; then
    cp "$BREW/lib/gio/modules/"libgio*.so \
       "$APP/Contents/Resources/lib/gio/modules/" 2>/dev/null || true
fi
# On macOS the extension may be .dylib rather than .so
if ls "$BREW/lib/gio/modules/"*.dylib &>/dev/null 2>&1; then
    cp "$BREW/lib/gio/modules/"*.dylib \
       "$APP/Contents/Resources/lib/gio/modules/" 2>/dev/null || true
fi

echo "==> Copying GDK-Pixbuf loaders..."
cp "$BREW/lib/gdk-pixbuf-2.0/2.10.0/loaders/"*.so \
   "$APP/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/" 2>/dev/null || \
cp "$BREW/lib/gdk-pixbuf-2.0/2.10.0/loaders/"*.dylib \
   "$APP/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/" 2>/dev/null || true
DYLD_LIBRARY_PATH="$BREW/lib" gdk-pixbuf-query-loaders \
    "$APP/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/"* \
    | sed "s|$(pwd)/$APP/Contents/Resources/||g" \
    > "$APP/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"

echo "==> Copying GStreamer plugins..."
GSTLIB="$BREW/lib/gstreamer-1.0"
for PLUGIN in coreelements playback audioconvert audioresample volume autodetect \
              coreaudio ogg vorbis theora opus matroska vpx \
              id3demux apetag icydemux mpegaudioparse; do
    cp "$GSTLIB/libgst${PLUGIN}.dylib" \
       "$APP/Contents/Resources/lib/gstreamer-1.0/" 2>/dev/null || true
done
# gst-plugin-scanner helper binary (lives under opt/gstreamer, not libexec)
GST_SCANNER="$BREW/opt/gstreamer/libexec/gstreamer-1.0/gst-plugin-scanner"
if [ -f "$GST_SCANNER" ]; then
    cp "$GST_SCANNER" "$APP/Contents/Resources/lib/gstreamer-1.0/"
fi

echo "==> Bundling dylibs (dylibbundler)..."
# Collect all native binaries that need rpath rewriting
EXTRA_BINS=()
while IFS= read -r -d '' f; do
    EXTRA_BINS+=("-x" "$f")
done < <(find "$APP/Contents/Resources/lib" \
              \( -name '*.dylib' -o -name '*.so' \) -print0)

SCANNER_ARGS=()
if [ -f "$APP/Contents/Resources/lib/gstreamer-1.0/gst-plugin-scanner" ]; then
    SCANNER_ARGS=("-x" "$APP/Contents/Resources/lib/gstreamer-1.0/gst-plugin-scanner")
fi

dylibbundler -od -b \
    -x "$APP/Contents/MacOS/Fractal" \
    "${EXTRA_BINS[@]}" \
    "${SCANNER_ARGS[@]}" \
    -d "$APP/Contents/Frameworks/" \
    -p @executable_path/../Frameworks/ \
    -s "$BREW/lib" \
    -s "$BREW/opt/gstreamer/lib" \
    -s "$BREW/opt/librsvg/lib"

echo "==> Creating ICNS icon..."
bash macos/create-icns.sh "$APP/Contents/Resources"

echo "==> Ad-hoc code signing..."
codesign --force --deep --sign - "$APP"

echo ""
echo "Done! $APP created."
echo ""
echo "To install:  cp -r $APP /Applications/"
echo "To launch:   open $APP"
