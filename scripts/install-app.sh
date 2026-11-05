#!/usr/bin/env bash
# Build the desktop app and install it to /Applications/Motetto.app (macOS).
# Usage: scripts/install-app.sh [--no-build]
# Signs with $APPLE_SIGNING_IDENTITY, else the first "Developer ID Application" identity in the
# keychain, so macOS keeps Screen Recording and Microphone permissions across installs.
# Needs the sidecars (scripts/build-sidecars.sh, once) and ui/node_modules (npm ci in ui/).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
app=$root/target/release/bundle/macos/Motetto.app
dest=/Applications/Motetto.app

if [[ ${1:-} != --no-build ]]; then
  if [[ -z ${APPLE_SIGNING_IDENTITY:-} ]]; then
    APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning |
      sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -n1)
  fi
  [[ -n $APPLE_SIGNING_IDENTITY ]] && echo "Signing with: $APPLE_SIGNING_IDENTITY"
  export APPLE_SIGNING_IDENTITY
  (cd "$root/app/src-tauri" && ../../ui/node_modules/.bin/tauri build --bundles app)
fi

[[ -d $app ]] || { echo "No build at $app" >&2; exit 1; }

# Never quit it here: a call may be recording. The user quits it.
while pgrep -f "$dest/Contents/MacOS/" >/dev/null; do
  read -r -p "Motetto is running. Quit it (stop any recording first), then press Enter. "
done

rm -rf "$dest"
ditto "$app" "$dest"
echo "Installed $dest"
open "$dest"
