#!/usr/bin/env bash
# Builds quickRS for macOS (universal: Apple Silicon and Intel) and packs it as a release (P20.4, D39):
#   dist/mac/quickRS.app                                 quickrs, with quickrs-cli and quickrs-convert beside it in
#                                                        Contents/MacOS; the icon in Contents/Resources
#   dist/mac/quickRS-<ver>-macos-universal.zip           quickRS.app at the zip's root (the updater's asset name)
#   dist/mac/quickRS-<ver>-macos-universal.dmg           the same, to drag into Applications (skip: NO_DMG=1)
#   dist/mac/SHA256SUMS.txt                              `<sha256>  <file>` for the zip and the dmg
# Runs on a Mac (the GitHub macOS runner: .github/workflows/release-mac.yml) with Rust (rustup) and Xcode or its
# Command Line Tools. Signed ad hoc (`codesign -s -`), not notarized: first open is right-click > Open.
# Based on the port that ran on the owner's Mac (2026-10-15, tools/package-mac.sh there).
#
#   tools/mac/bundle.sh                                     the public build (no `nux` feature)
#   FEATURES="qrs-app/nux qrs-cli/nux" tools/mac/bundle.sh  with the NUX MG-300 tile (private repo only)
set -euo pipefail
cd "$(dirname "$0")/../.."
root="$(pwd)"

export PATH="$HOME/.cargo/bin:$PATH"
# Prefer the compiler and Cargo from the active rustup toolchain over unrelated system installs (for example, Homebrew
# Rust). `rustup target add` installs targets into this toolchain, so Cargo must use its matching rustc as well.
rustup_toolchain_bin="$(dirname "$(rustup which cargo)")"
export PATH="$rustup_toolchain_bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

# signalsmith-stretch's bindgen needs libclang (the private repo's .cargo/config.toml names the Windows one; an
# environment variable wins over it)
find_libclang() {
    local dev
    dev="$(xcode-select -p 2>/dev/null || true)"
    for d in "${LIBCLANG_PATH:-}" \
        "$dev/Toolchains/XcodeDefault.xctoolchain/usr/lib" \
        "$dev/usr/lib" \
        /Library/Developer/CommandLineTools/usr/lib \
        "$(brew --prefix llvm 2>/dev/null || echo /nonexistent)/lib"; do
        if [ -n "$d" ] && [ -f "$d/libclang.dylib" ]; then
            echo "$d"
            return
        fi
    done
    echo "libclang.dylib not found: install Xcode or its Command Line Tools (xcode-select --install)" >&2
    exit 1
}
LIBCLANG_PATH="$(find_libclang)"
export LIBCLANG_PATH

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$version" ] || { echo "no version in Cargo.toml" >&2; exit 1; }
features="${FEATURES:-}"
echo "quickRS $version for macOS (features: ${features:-none}, macOS $MACOSX_DEPLOYMENT_TARGET+)"

# both architectures, then one universal binary per program
targets=(aarch64-apple-darwin x86_64-apple-darwin)
bins=(quickrs quickrs-cli quickrs-convert)
rustup target add "${targets[@]}" >/dev/null
for t in "${targets[@]}"; do
    cargo build --release --target "$t" -p qrs-app -p qrs-cli -p qrs-convert \
        --no-default-features ${features:+--features "$features"}
done
uni=target/universal-apple-darwin/release
mkdir -p "$uni"
for b in "${bins[@]}"; do
    lipo -create -output "$uni/$b" "target/${targets[0]}/release/$b" "target/${targets[1]}/release/$b"
    lipo -info "$uni/$b"
done

out=dist/mac
app="$out/quickRS.app"
rm -rf "$out"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for b in "${bins[@]}"; do
    cp "$uni/$b" "$app/Contents/MacOS/$b"
done

# the icon: an iconset from the 1024 px png (sips + iconutil), else the committed .icns
icns="$app/Contents/Resources/quickRS.icns"
make_icns() {
    local set="$1/quickRS.iconset" s
    mkdir -p "$set"
    for s in 16 32 128 256 512; do
        sips -z $s $s assets/icons/rsplay.png --out "$set/icon_${s}x${s}.png" >/dev/null || return 1
        sips -z $((s * 2)) $((s * 2)) assets/icons/rsplay.png --out "$set/icon_${s}x${s}@2x.png" >/dev/null || return 1
    done
    iconutil -c icns "$set" -o "$icns"
}
tmp="$(mktemp -d)"
if make_icns "$tmp"; then
    echo "icon made from assets/icons/rsplay.png"
else
    cp assets/icons/rsplay.icns "$icns"
fi
rm -rf "$tmp"

# run-time assets (the MG-300 pictures; fonts and icons are compiled in), only for a build that uses them
if [[ "$features" == *nux* ]] && [ -d assets/mg300 ]; then
    mkdir -p "$app/Contents/Resources/assets"
    cp -R assets/mg300 "$app/Contents/Resources/assets/"
fi

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>quickRS</string>
  <key>CFBundleDisplayName</key><string>quickRS Bristol 2026</string>
  <key>CFBundleIdentifier</key><string>io.github.arpan2711.quickrs</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleExecutable</key><string>quickrs</string>
  <key>CFBundleIconFile</key><string>quickRS</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>quickRS listens to your guitar (an audio interface, a Real Tone Cable or the microphone) to hear the notes you play.</string>
  <key>NSHumanReadableCopyright</key><string>quickRS, MIT licence. Not affiliated with Ubisoft.</string>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

# ad-hoc signature: the helpers first, then the bundle (so Gatekeeper and the microphone prompt see one app)
for b in quickrs-cli quickrs-convert; do
    codesign --force --sign - --timestamp=none "$app/Contents/MacOS/$b"
done
codesign --force --sign - --timestamp=none "$app"
codesign --verify --deep --strict --verbose=2 "$app"

zip_name="quickRS-$version-macos-universal.zip"
dmg_name="quickRS-$version-macos-universal.dmg"
(cd "$out" && ditto -c -k --sequesterRsrc --keepParent quickRS.app "$zip_name")
sums=("$zip_name")
if [ -z "${NO_DMG:-}" ]; then
    stage="$(mktemp -d)"
    ditto "$app" "$stage/quickRS.app"
    ln -s /Applications "$stage/Applications"
    hdiutil create -volname "quickRS $version" -srcfolder "$stage" -ov -format UDZO "$out/$dmg_name" >/dev/null
    rm -rf "$stage"
    sums+=("$dmg_name")
fi
(cd "$out" && shasum -a 256 "${sums[@]}" > SHA256SUMS.txt && cat SHA256SUMS.txt)
echo "built $root/$out"
