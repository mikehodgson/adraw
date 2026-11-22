# ADraw

ADraw is a Windows desktop editor for ANSI art. Draw with character brushes,
type directly on the canvas, and add lettering with TheDraw fonts.

![ADraw displaying its application icon as truecolor ANSI artwork](docs/images/adraw-screenshot.jpg)

The icon recreated as 64 x 36 truecolor ANSI art.
[Open the example in ADraw](examples/adraw-icon.ans).

## Get started

1. Download and run the Windows installer from the
   **[latest release](https://github.com/mikehodgson/adraw/releases/latest)**.
   Or extract the portable ZIP and double-click `adraw.exe`.
2. Choose **File > New image** to start an **80 x 25** canvas.
3. Pick a character and colors in the sidebar. **Left-click** a color for
   foreground; **right-click** for background.
4. Click or drag on the canvas to draw, or click a cell and type.
5. Choose **File > Save as...** to save your artwork as an `.ans` file.

For custom image sizes, editing shortcuts, and step-by-step feature
instructions, read the **[User guide](USER_GUIDE.md)**.

Requires 64-bit Windows 10 or 11. The installer needs no administrator rights.
Close ADraw before upgrading. Releases are not code-signed, so Windows may show
a SmartScreen warning; download from the official release page.

## Main features

- Custom canvas sizes, character brushes, and direct keyboard editing.
- CP437 and UTF-8 support, with a bundled IBM VGA 9x16 display font.
- 16-color, 256-color, and truecolor palettes, plus swatches of artwork colors.
- Selections, drag-to-move, styled copy/paste, undo/redo, and selection-only saves.
- A persistent TheDraw TDF font library with searchable selection and text previews.
- Proportional zoom and a reorderable, collapsible sidebar.
- ANSI file loading/saving and SAUCE artwork credits.

## Learn more

- **[User guide](USER_GUIDE.md):** detailed instructions and file compatibility.
- [Changelog](CHANGELOG.md): unreleased changes and release history.
- [Developer guide](https://github.com/mikehodgson/adraw/blob/main/DEVELOPERS.md):
  source builds, testing, architecture, and credits.

Code is MIT-licensed. The bundled IBM VGA font is by VileR, licensed under
CC BY-SA 4.0; see the included license notices.
