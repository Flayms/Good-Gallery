#!/bin/sh
# Smoke test of a Linux package in a clean system (REQ-018-12, linux-packages.yml): installs it with the system's
# package manager, which brings the helpers, starts the app once with a screenshot, checks that it found the helpers,
# then removes the package and checks that the data folder stays.
#
#   ./packaging/linux/smoke-test.sh target/dist/good-gallery-<version>-linux-x64.{deb,rpm}
set -eu

check() {
  # $1 is a test expression, $2 what it means.
  if eval "$1"; then
    echo "ok: $2"
  else
    echo "Smoke test failed: $2" >&2
    exit 1
  fi
}

package=$(realpath "$1")
data=$(mktemp -d)/data
if command -v apt-get > /dev/null; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -q
  apt-get install -qy "$package" xvfb xauth mesa-vulkan-drivers
  remove="apt-get remove -qy good-gallery"
else
  dnf install -qy "$package" xorg-x11-server-Xvfb xauth mesa-vulkan-drivers
  remove="dnf remove -qy good-gallery"
fi

for file in /usr/bin/good-gallery /usr/share/applications/good-gallery.desktop \
  /usr/share/icons/hicolor/256x256/apps/good-gallery.png /usr/share/doc/good-gallery/LICENSE.txt; do
  check "[ -e $file ]" "installed $file"
done
check "grep -q '^MimeType=.*image/jpeg' /usr/share/applications/good-gallery.desktop" "the menu entry opens photos"
for helper in exiftool ffmpeg; do
  check "command -v $helper > /dev/null" "the package brought $helper"
done

xvfb-run -a good-gallery --data-dir "$data" --screenshot "$data/screenshot.png" --screenshot-after 5
check "[ -s '$data/screenshot.png' ]" "the app saved its screenshot"
cat "$data/logs/app.log"
for helper in exiftool ffmpeg libmpv; do
  check "grep -q '$helper: ' '$data/logs/app.log'" "the log finds $helper"
done
check "! grep -q ' ERROR ' '$data/logs/app.log'" "the log has no errors"

$remove
check "[ ! -e /usr/bin/good-gallery ]" "the app is removed"
check "[ ! -e /usr/share/applications/good-gallery.desktop ]" "the menu entry is removed"
check "[ -e '$data/gallery.db' ]" "the data folder stays"
echo "Smoke test passed."
