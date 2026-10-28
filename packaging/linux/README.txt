Good Gallery for Linux

Good Gallery uses your distribution's exiftool, ffmpeg and libmpv. Install them first, e.g. on Ubuntu and Debian:

  sudo apt install libimage-exiftool-perl ffmpeg libmpv2

Fedora: sudo dnf install perl-Image-ExifTool ffmpeg mpv-libs (ffmpeg from RPM Fusion)
Arch: sudo pacman -S perl-image-exiftool ffmpeg mpv

Then either run ./good-gallery from this folder, or install it for your user (app menu entry included):

  ./install.sh              installs into ~/.local
  ./install.sh --uninstall  removes it again

The index, settings and thumbnail cache are kept in ~/.local/share/good-gallery ($XDG_DATA_HOME/good-gallery).
