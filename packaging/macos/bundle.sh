#!/bin/sh
# Builds dist/<package>.zip holding Axe-Grid.app (arm64 + x86_64) and the CLI.
# usage: packaging/macos/bundle.sh <version> <package-name>
# Expects release builds for both Apple targets in target/.
set -eu

version="$1"
package="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
stage="$root/dist/$package"
app="$stage/Axe-Grid.app"

rm -rf "$stage"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

for binary in axegrid axegrid-cli; do
    lipo -create -output "$stage/$binary" \
        "$root/target/aarch64-apple-darwin/release/$binary" \
        "$root/target/x86_64-apple-darwin/release/$binary"
done
mv "$stage/axegrid" "$app/Contents/MacOS/axegrid"
chmod +x "$app/Contents/MacOS/axegrid" "$stage/axegrid-cli"

sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$root/LICENSE" "$root/ATTRIBUTION.md" "$stage/"
cp "$root/docs/install.md" "$stage/INSTALL.md"

# Ad-hoc signatures: Apple silicon will not run unsigned code. This is not
# a Developer ID signature, so Gatekeeper still asks on first launch.
codesign --force --sign - "$stage/axegrid-cli"
codesign --force --deep --sign - "$app"

(cd "$root/dist" && ditto -c -k --keepParent "$package" "$package.zip")
rm -rf "$stage"
