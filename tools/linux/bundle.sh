#!/usr/bin/env bash
# Builds quickRS for Linux (x86_64) and packs it as a release (P28.4):
#   dist/linux/quickRS-<ver>-linux-x86_64.AppImage   one file: make it executable and run it. `<it> cli ...` runs
#                                                    quickrs-cli, `<it> convert` the converter
#   dist/linux/quickRS-<ver>-linux-x86_64.tar.gz     the same programs in a folder `quickRS-<ver>/` (no FUSE needed)
#   dist/linux/SHA256SUMS.txt                        `<sha256>  <file>` for both
# Build it on the oldest Linux it should run on: the programs need the glibc they were built against or newer
# (Ubuntu 22.04: glibc 2.35, so Ubuntu 22.04+, Mint 21+, Debian 12, Fedora 36+, current Arch). Needs Rust (rustup),
# build-essential, pkg-config, libasound2-dev, clang/libclang-dev, curl and file. The AppImage is made by
# appimagetool (downloaded once into target/, pinned); with no FUSE it runs extracted (APPIMAGE_EXTRACT_AND_RUN).
#
#   tools/linux/bundle.sh                 the public build (no default features: no NUX MG-300, no Lessons, no
#                                         brand names)
#   LABEL=preview1 tools/linux/bundle.sh  the files named quickRS-preview1-linux-x86_64.* (a test build between
#                                         releases; the app still says its Cargo version)
set -euo pipefail
cd "$(dirname "$0")/../.."
root="$(pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

# signalsmith-stretch's bindgen needs libclang (the private repo's .cargo/config.toml names the Windows one; an
# environment variable wins over it)
if [ -z "${LIBCLANG_PATH:-}" ] || [ ! -d "$LIBCLANG_PATH" ]; then
    lib="$(ls /usr/lib/llvm-*/lib/libclang.so* /usr/lib64/libclang.so* /usr/lib/libclang.so* 2>/dev/null | head -1 || true)"
    [ -n "$lib" ] || { echo "libclang not found: install libclang-dev (or clang-devel)" >&2; exit 1; }
    LIBCLANG_PATH="$(dirname "$lib")"
fi
export LIBCLANG_PATH

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$version" ] || { echo "no version in Cargo.toml" >&2; exit 1; }
glibc=$(ldd --version 2>&1 | sed -n '1s/.* //p')
echo "quickRS $version for Linux x86_64 (no default features, glibc $glibc)"

bins=(quickrs quickrs-cli quickrs-convert)
cargo build --release --no-default-features -p qrs-app -p qrs-cli -p qrs-convert

out=dist/linux
rm -rf "$out"
mkdir -p "$out"

# the AppDir: the programs, the desktop entry and icon, and AppRun (which picks the program)
app="$out/quickRS.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
for b in "${bins[@]}"; do
    install -m 755 "target/release/$b" "$app/usr/bin/$b"
    strip "$app/usr/bin/$b" 2>/dev/null || true
done
cat > "$app/quickrs.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=quickRS
GenericName=Guitar practice player
Comment=Play your own RS2014 songs with your real guitar
Exec=quickrs %f
Icon=quickrs
Terminal=false
Categories=AudioVideo;Audio;Music;Education;
Keywords=guitar;bass;rocksmith;tab;practice;
StartupWMClass=quickrs
X-AppImage-Version=$version
EOF
cp "$app/quickrs.desktop" "$app/usr/share/applications/quickrs.desktop"
# 256 px, as the window's own icon
python3 - "$app" <<'EOF' 2>/dev/null || cp assets/icons/rsplay.png "$app/quickrs.png"
import sys
from PIL import Image
Image.open("assets/icons/rsplay.png").convert("RGBA").resize((256, 256), Image.LANCZOS).save(sys.argv[1] + "/quickrs.png")
EOF
cp "$app/quickrs.png" "$app/usr/share/icons/hicolor/256x256/apps/quickrs.png"
ln -sf quickrs.png "$app/.DirIcon"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
# quickRS's AppImage: `quickRS.AppImage` opens quickRS; `quickRS.AppImage cli ...` runs quickrs-cli (devices,
# play, convert-folder...); `quickRS.AppImage convert` opens the song converter.
here="$(dirname "$(readlink -f "$0")")"
case "${1:-}" in
    cli) shift; exec "$here/usr/bin/quickrs-cli" "$@" ;;
    convert) shift; exec "$here/usr/bin/quickrs-convert" "$@" ;;
    *) exec "$here/usr/bin/quickrs" "$@" ;;
esac
EOF
chmod 755 "$app/AppRun"
command -v desktop-file-validate >/dev/null && desktop-file-validate "$app/quickrs.desktop"

# appimagetool, pinned (1.9.1, 2025-11-18)
tool=target/appimagetool-1.9.1-x86_64.AppImage
if [ ! -x "$tool" ]; then
    curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
    chmod 755 "$tool"
fi
name="${LABEL:-$version}"
image="quickRS-$name-linux-x86_64.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$app" "$out/$image"

# the same programs as a plain folder, for a system without FUSE (and to look inside)
tarname="quickRS-$name-linux-x86_64.tar.gz"
stage="$out/quickRS-$name"
mkdir -p "$stage"
cp "$app"/usr/bin/* "$stage/"
cp "$app/quickrs.desktop" "$app/quickrs.png" "$stage/"
cp tools/linux/README-linux.txt "$stage/README.txt"
tar -C "$out" -czf "$out/$tarname" "quickRS-$name"
rm -rf "$stage" "$app"

(cd "$out" && sha256sum "$image" "$tarname" > SHA256SUMS.txt && cat SHA256SUMS.txt)
for b in "${bins[@]}"; do
    echo "$b needs glibc $(objdump -T "target/release/$b" | grep -oE 'GLIBC_[0-9.]+' | sort -V | tail -1 | cut -d_ -f2)"
done
echo "done: $root/$out"
