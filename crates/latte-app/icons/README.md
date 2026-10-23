# App icon

`icon.svg` is the source: a mug that is also a book, with steam. The PNG, `.ico` and
`.icns` files are made from it at the sizes each platform needs:

- `32x32.png`, `128x128.png`, `128x128@2x.png` (256) and `icon.png` (512) for Linux;
- `icon.ico` (16 to 256, PNG entries) for Windows, also built into the `.exe`;
- `icon.icns` (16 to 1024, PNG entries) for macOS.

Change the SVG, then make the others again from it at those sizes.
