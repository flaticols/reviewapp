APP_NAME  := Switchfold Review
BUNDLE_ID := dev.switchfold.review
VERSION   := $(shell sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p;}' Cargo.toml)
APP       := target/release/$(APP_NAME).app
INSTALLED := /Applications/$(APP_NAME).app

export APP_NAME BUNDLE_ID VERSION

.PHONY: run build app install clean

## run: launch the app from source
run:
	cargo run -p app_ui --release

## build: release binary only
build:
	cargo build --release -p app_ui

## app: build the signed .app bundle in target/release
app: build
	rm -rf "$(APP)"
	mkdir -p "$(APP)/Contents/MacOS" "$(APP)/Contents/Resources"
	cp target/release/switchfold "$(APP)/Contents/MacOS/switchfold"
	xcrun actool assets/AppIcon.icon --compile "$(APP)/Contents/Resources" \
		--platform macosx --minimum-deployment-target 13.0 --app-icon AppIcon \
		--output-partial-info-plist "$(APP)/../icon-partial.plist" \
		--output-format human-readable-text >/dev/null
	rm -f "$(APP)/../icon-partial.plist"
	scripts/info-plist.sh > "$(APP)/Contents/Info.plist"
	codesign --force --sign - --timestamp=none "$(APP)"

## install: build the .app and copy it to /Applications, replacing an earlier copy
install: app
	rm -rf "$(INSTALLED)"
	cp -R "$(APP)" /Applications/
	@echo "Installed $(INSTALLED)"

clean:
	cargo clean
