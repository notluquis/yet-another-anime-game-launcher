#!/bin/bash
set -e

APP_NAME="Yaagl-OS"
BINARY_NAME="Yaagl"
BUNDLE_ID="moe.reimu.app.os"

echo "🏗️  Creating ${APP_NAME}.app bundle..."

# Clean old app
rm -rf "${APP_NAME}.app"

# Create app structure
mkdir -p "${APP_NAME}.app/Contents/MacOS"
mkdir -p "${APP_NAME}.app/Contents/Resources/.storage"

# Copy binary
cp "dist/${BINARY_NAME}/${BINARY_NAME}-mac_arm64" "${APP_NAME}.app/Contents/MacOS/${BINARY_NAME}"
chmod +x "${APP_NAME}.app/Contents/MacOS/${BINARY_NAME}"

# Copy resources
cp "dist/${BINARY_NAME}/resources.neu" "${APP_NAME}.app/Contents/Resources/"

# Copy sidecar directory
echo "📦 Copying sidecar..."
rsync -a --exclude='.git' --exclude='node_modules' --exclude='*.pyc' sidecar/ "${APP_NAME}.app/Contents/Resources/sidecar/"

# Copy Sophon if exists
if [ -f "sophon_server/dist/sophon-server" ]; then
    echo "📦 Copying Sophon server..."
    mkdir -p "${APP_NAME}.app/Contents/Resources/sidecar/sophon_server"
    cp "sophon_server/dist/sophon-server" "${APP_NAME}.app/Contents/Resources/sidecar/sophon_server/"
    chmod +x "${APP_NAME}.app/Contents/Resources/sidecar/sophon_server/sophon-server"
fi

# Make all files in sidecar executable if they have no extension
find "${APP_NAME}.app/Contents/Resources/sidecar" -type f ! -name "*.*" -exec chmod +x {} \;

# Create launcher script
cat > "${APP_NAME}.app/Contents/MacOS/launcher" << 'EOF'
#!/usr/bin/env bash
SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
APST_DIR="$HOME/Library/Application Support/Yaagl-OS"
mkdir -p "$APST_DIR"
CONTENTS_DIR="$(dirname "$SCRIPT_DIR")"
rsync -rlptu "$CONTENTS_DIR/Resources/." "$APST_DIR"
cd "$APST_DIR"
exec "$SCRIPT_DIR/Yaagl" --path="$APST_DIR" 2>&1 | tee "$APST_DIR/app.log"
EOF

chmod +x "${APP_NAME}.app/Contents/MacOS/launcher"

# Create Info.plist
cat > "${APP_NAME}.app/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleExecutable</key>
    <string>launcher</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleName</key>
    <string>Yaagl OS</string>
    <key>CFBundleDisplayName</key>
    <string>Yaagl OS</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15.0</string>
    <key>NSAppTransportSecurity</key>
    <dict>
        <key>NSAllowsArbitraryLoads</key>
        <true/>
    </dict>
</dict>
</plist>
EOF

echo "✅ Bundle created: ${APP_NAME}.app"
du -sh "${APP_NAME}.app"
