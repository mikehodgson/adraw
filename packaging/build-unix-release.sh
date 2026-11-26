#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
target="${1:?Usage: bash packaging/build-unix-release.sh TARGET}"
case "$(uname -s):$(uname -m):$target" in
    Linux:x86_64:x86_64-unknown-linux-gnu) platform=linux-x64 ;;
    Linux:aarch64:aarch64-unknown-linux-gnu) platform=linux-arm64 ;;
    Darwin:x86_64:x86_64-apple-darwin) platform=macos-x64 ;;
    Darwin:arm64:aarch64-apple-darwin) platform=macos-arm64 ;;
    *) echo "Unsupported native host/target combination: $target" >&2; exit 1 ;;
esac
version="$(cargo pkgid --locked -p adraw)"
version="${version##*@}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
    echo "Unsupported release version: $version" >&2
    exit 1
fi
if [[ "${GITHUB_REF_TYPE:-}" == tag && "${GITHUB_REF_NAME:-}" != "$version" && "${GITHUB_REF_NAME:-}" != "v$version" ]]; then
    echo "Tag ${GITHUB_REF_NAME:-} does not match Cargo version $version." >&2
    exit 1
fi

cargo build --locked --release -p adraw --target "$target"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
dist="$root/target/dist"
mkdir -p "$dist"
stage="$(mktemp -d "$dist/.stage-XXXXXXXX")"
trap 'rm -rf -- "$stage"' EXIT
name="adraw-$version-$platform"
payload="$stage/$name"
mkdir -p "$payload/docs/images" "$payload/examples"
cp README.md USER_GUIDE.md CHANGELOG.md NOTICE.txt "$payload/"
cp LICENSE "$payload/LICENSE.txt"
cp crates/ansi_editor/assets/LICENSE-IBM-VGA.txt "$payload/"
cp docs/images/adraw-screenshot.jpg "$payload/docs/images/"
cp examples/adraw-icon.ans "$payload/examples/"

if [[ "$platform" == macos-* ]]; then
    contents="$payload/ADraw.app/Contents"
    mkdir -p "$contents/MacOS" "$contents/Resources" "$stage/adraw.iconset"
    cp "$target_dir/$target/release/adraw" "$contents/MacOS/adraw"
    chmod 755 "$contents/MacOS/adraw"
    cat > "$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>adraw</string>
<key>CFBundleIdentifier</key><string>io.github.mikehodgson.adraw</string>
<key>CFBundleName</key><string>ADraw</string>
<key>CFBundleDisplayName</key><string>ADraw</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>${version%%[-+]*}</string>
<key>CFBundleVersion</key><string>${version%%[-+]*}</string>
<key>CFBundleIconFile</key><string>adraw.icns</string>
<key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET:-11.0}</string>
<key>NSHighResolutionCapable</key><true/>
<key>CFBundleDocumentTypes</key><array><dict>
<key>CFBundleTypeName</key><string>ANSI art</string>
<key>CFBundleTypeRole</key><string>Editor</string>
<key>LSHandlerRank</key><string>Alternate</string>
<key>LSItemContentTypes</key><array><string>io.github.mikehodgson.adraw.ansi-art</string></array>
</dict></array>
<key>UTImportedTypeDeclarations</key><array><dict>
<key>UTTypeIdentifier</key><string>io.github.mikehodgson.adraw.ansi-art</string>
<key>UTTypeDescription</key><string>ANSI art</string>
<key>UTTypeConformsTo</key><array><string>public.plain-text</string></array>
<key>UTTypeTagSpecification</key><dict>
<key>public.filename-extension</key><array><string>ans</string><string>asc</string></array>
</dict>
</dict></array>
</dict></plist>
EOF
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" crates/ansi_editor/assets/adraw.png \
            --out "$stage/adraw.iconset/icon_${size}x${size}.png" > /dev/null
        double=$((size * 2))
        sips -z "$double" "$double" crates/ansi_editor/assets/adraw.png \
            --out "$stage/adraw.iconset/icon_${size}x${size}@2x.png" > /dev/null
    done
    iconutil -c icns "$stage/adraw.iconset" -o "$contents/Resources/adraw.icns"
    plutil -lint "$contents/Info.plist"
    # Replacing the icon/plist invalidates any linker-generated ad-hoc signature.
    codesign --force --sign - "$payload/ADraw.app"
    codesign --verify --deep --strict "$payload/ADraw.app"
else
    cp "$target_dir/$target/release/adraw" "$payload/adraw"
    chmod 755 "$payload/adraw"
    cp packaging/linux-install.sh "$payload/install.sh"
    chmod 755 "$payload/install.sh"
    cp crates/ansi_editor/assets/adraw.png "$payload/adraw.png"
fi

archive="$dist/$name.tar.gz"
tar -czf "$archive" -C "$stage" "$name"
(
    cd "$dist"
    if [[ "$platform" == macos-* ]]; then
        shasum -a 256 "$name.tar.gz"
    else
        sha256sum "$name.tar.gz"
    fi
) > "$dist/SHA256SUMS.txt"
echo "Release $version is ready: $archive"
