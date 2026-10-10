#!/bin/sh
# Build Tetris.app: the readable game with your ROM built in (see build.rs), launchable from
# Finder, Launchpad or Spotlight. Output: target/Tetris.app (gitignored; it contains your ROM).
#
#     tetris-rs/macos/bundle.sh             # build target/Tetris.app
#     tetris-rs/macos/bundle.sh --install   # ...and copy it to ~/Applications
set -eu
cd "$(dirname "$0")/../.."
cargo build --release -p tetris-rs --bin tetris

app=target/Tetris.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/tetris "$app/Contents/MacOS/tetris"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Tetris</string>
    <key>CFBundleDisplayName</key><string>Tetris</string>
    <key>CFBundleIdentifier</key><string>local.tetris-nes.readable</string>
    <key>CFBundleExecutable</key><string>tetris</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Built locally, so Gatekeeper doesn't quarantine it; sign ad hoc for the bundle as a whole.
codesign --force --sign - "$app" >/dev/null 2>&1 || true
echo "built $app"

if [ "${1:-}" = "--install" ]; then
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/Tetris.app"
    cp -R "$app" "$HOME/Applications/"
    echo "installed $HOME/Applications/Tetris.app"
fi
