# Icon Generation

To generate the required icon files, you can use ImageMagick:

```bash
# Install ImageMagick if not already installed
# Ubuntu/Debian: sudo apt install imagemagick
# Fedora: sudo dnf install ImageMagick
# Arch: sudo pacman -S imagemagick

# Generate PNG icons from SVG
convert -background transparent icon.svg -resize 32x32 32x32.png
convert -background transparent icon.svg -resize 128x128 128x128.png
convert -background transparent icon.svg -resize 256x256 128x128@2x.png
convert -background transparent icon.svg -resize 128x128 icon.png
```

Alternatively, use an online tool like:
- https://realfavicongenerator.net/
- https://cloudconvert.com/svg-to-png

This project only bundles Linux icon assets. Replace the placeholder PNG and SVG files with properly generated Linux icons.

