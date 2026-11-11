#!/usr/bin/env nu
# Builds `Switchfold Review.app` from a release build: the `switchfold` binary, the
# Icon Composer icon compiled with `actool`, and an Info.plist, signed ad hoc.
#
#   nu scripts/bundle.nu             # target/release/Switchfold Review.app
#   nu scripts/bundle.nu --install   # also replaces /Applications/Switchfold Review.app

const APP_NAME = "Switchfold Review"
const BUNDLE_ID = "dev.switchfold.review"

def main [
    --install # Copy the app into /Applications, replacing an earlier copy
] {
    let root = $env.FILE_PWD | path dirname
    cd $root

    print "Building the release binary…"
    cargo build --release -p app_ui

    let version = open Cargo.toml | get workspace.package.version
    let app = $root | path join target release $"($APP_NAME).app"
    let contents = $app | path join Contents
    let resources = $contents | path join Resources
    rm -rf $app
    mkdir ($contents | path join MacOS) $resources
    cp ($root | path join target release switchfold) ($contents | path join MacOS switchfold)

    print "Compiling the icon…"
    let partial = mktemp -t switchfold-icon.XXXXXX.plist
    (^xcrun actool assets/AppIcon.icon
        --compile $resources
        --platform macosx
        --minimum-deployment-target 13.0
        --app-icon AppIcon
        --output-partial-info-plist $partial
        --output-format human-readable-text) | ignore
    rm $partial

    info_plist $version | save ($contents | path join Info.plist)

    print "Signing…"
    ^codesign --force --sign - --timestamp=none $app

    if $install {
        let installed = $"/Applications/($APP_NAME).app"
        rm -rf $installed
        cp -r $app /Applications/
        print $"Installed ($installed)"
    } else {
        print $"Built ($app)"
    }
}

# The bundle's Info.plist for `version`.
def info_plist [version: string] {
    $'<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>($APP_NAME)</string>
    <key>CFBundleDisplayName</key><string>($APP_NAME)</string>
    <key>CFBundleIdentifier</key><string>($BUNDLE_ID)</string>
    <key>CFBundleExecutable</key><string>switchfold</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>($version)</string>
    <key>CFBundleVersion</key><string>($version)</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleIconName</key><string>AppIcon</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>($BUNDLE_ID)</string>
            <key>CFBundleURLSchemes</key><array><string>($BUNDLE_ID)</string></array>
        </dict>
    </array>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
'
}
