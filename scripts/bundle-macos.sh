#!/usr/bin/env bash
# Usage: scripts/bundle-macos.sh <binary> <version> <output.dmg>
#
# Wraps the binary in Astrolabe.app, signs it "ad hoc" (no Apple Developer account, so no
# notarization: see the README for opening it the first time), then puts it in a .dmg
# with a shortcut to Applications. Run it on macOS.
set -euo pipefail
cd "$(dirname "$0")/.."

bin=${1:?usage: scripts/bundle-macos.sh <binary> <version> <output.dmg>}
ver=${2:?}
out=${3:?}

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app="$stage/Astrolabe.app/Contents"
mkdir -p "$app/MacOS" "$app/Resources"
install -m755 "$bin" "$app/MacOS/astrolabe"
cp packaging/macos/Astrolabe.icns "$app/Resources/Astrolabe.icns"
cp LICENSE assets/fonts/LICENSE-IBM-Plex.txt "$app/Resources/"
sed "s/@VERSION@/$ver/g" packaging/macos/Info.plist >"$app/Info.plist"
plutil -lint "$app/Info.plist" >/dev/null
codesign --force --sign - "$stage/Astrolabe.app"
ln -s /Applications "$stage/Applications"

mkdir -p "$(dirname "$out")"
# hdiutil sometimes fails at random ("Resource busy") on the runners: three tries.
for try in 1 2 3; do
    hdiutil create -volname Astrolabe -srcfolder "$stage" -ov -format UDZO "$out" >/dev/null && break
    [ "$try" = 3 ] && exit 1
    sleep 5
done
echo "bundled $out"
