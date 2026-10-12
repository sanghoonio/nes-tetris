#!/bin/sh
# Build Tetris.app, launchable from Finder, Launchpad or Spotlight. It asks for the ROM on first
# launch and keeps it; nothing of the game is in the app, so it can be shared. Output:
# target/Tetris.app (gitignored).
#
#     desktop/macos/bundle.sh                 # build target/Tetris.app
#     desktop/macos/bundle.sh --install       # ...and copy it to ~/Applications
#     desktop/macos/bundle.sh --versus ...    # with VERSUS (online matches)
#     desktop/macos/bundle.sh --embed-rom ... # your ROM's data and graphics built in (build.rs),
#                                             # so it never asks; don't share this one
set -eu
cd "$(dirname "$0")/../.."
install=no
features=""
for arg in "$@"; do
    case "$arg" in
        --install) install=yes ;;
        --versus) features="$features versus" ;;
        --embed-rom) features="$features embedded-rom" ;;
        *) echo "unknown option $arg" >&2; exit 2 ;;
    esac
done
cargo build --release -p desktop --bin tetris --features "$features"

app=target/Tetris.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/tetris "$app/Contents/MacOS/tetris"
# The icon: the red Z in the playfield's frame (icon.py).
uv run --quiet --with pillow python3 desktop/macos/icon.py target/icon --piece z
iconutil -c icns target/icon/Tetris.iconset -o "$app/Contents/Resources/Tetris.icns"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Tetris</string>
    <key>CFBundleDisplayName</key><string>Tetris</string>
    <key>CFBundleIdentifier</key><string>local.tetris-nes.readable</string>
    <key>CFBundleExecutable</key><string>tetris</string>
    <key>CFBundleIconFile</key><string>Tetris</string>
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
# Rebuilt in place, the app can keep Finder's old (or no) icon: tell Launch Services it changed.
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
refresh() { touch "$1"; "$lsregister" -f "$1" 2>/dev/null || true; }
refresh "$app"
echo "built $app"

if [ "$install" = yes ]; then
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/Tetris.app"
    cp -R "$app" "$HOME/Applications/"
    refresh "$HOME/Applications/Tetris.app"
    echo "installed $HOME/Applications/Tetris.app"
fi
