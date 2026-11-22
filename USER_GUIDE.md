# ADraw user guide

This guide explains ADraw's editing tools and file workflows. For installation
and a quick start, see the [README](README.md).

The Windows installer installs for your current user, normally in
`%LOCALAPPDATA%\Programs\ADraw`, and adds a Start menu shortcut. A desktop
shortcut is optional. Close ADraw before installing an update.
Uninstall through **Windows Settings > Apps**; uninstall and upgrades leave
your saved artwork, preferences, and imported fonts intact.
The portable ZIP requires no installation; extract it and run `adraw.exe`.
Both distributions include the display font, documentation, and license notices.

## Contents

- [Create and resize images](#create-and-resize-images)
- [Draw and type](#draw-and-type)
- [Colors and attributes](#colors-and-attributes)
- [Selections and clipboard](#selections-and-clipboard)
- [Open and save artwork](#open-and-save-artwork)
- [SAUCE artwork credits](#sauce-artwork-credits)
- [Zoom and scrolling](#zoom-and-scrolling)
- [Arrange the sidebar](#arrange-the-sidebar)
- [Lettering with TheDraw fonts](#lettering-with-thedraw-fonts)
- [Encoding and compatibility](#encoding-and-compatibility)

## Create and resize images

Choose **File > New image** for the default **80 x 25** canvas.
For a custom size, set **Width** and **Height** in the **Canvas** menu before
creating the image.

To resize an existing image, set the new dimensions in **Canvas**, then choose
**Resize canvas**. Use **Edit > Undo** or **Ctrl+Z** to undo a resize.

The startup window fits the default canvas and sidebar. You can resize the
window; larger or zoomed artwork scrolls rather than changing the image size.

## Draw and type

Choose a character in the brush input or open the **CP437 character picker**
beside it. CP437 characters use the bundled **IBM VGA 9x16** font; no font
installation is needed.

Choose **Draw** in the sidebar, then click or drag on the canvas to paint.
To type directly, click a cell and enter text. Typed characters use your brush
colors and attributes.

| Key | Action |
| --- | --- |
| Arrow keys | Move the cursor. |
| Enter | Start another row. |
| Backspace / Delete | Erase characters. |
| Ctrl+Z / Ctrl+Y | Undo / redo. |

Hover sidebar icons to see their labels and keyboard shortcuts.

## Colors and attributes

All palettes are shared: **left-click** a color to set foreground,
**right-click** to set background. The **FG** and **BG** swatches show the
current brush colors.

- **16 colors:** choose from the inline palette.
- **256 colors:** open the indexed palette with the 256-color button, then
  left/right-click a swatch for foreground/background.
- **Truecolor:** left/right-click the RGB button or either current-color
  swatch to open the foreground/background RGB picker. Opening a picker does
  not change the selected color until you choose another.

The **Used colors** palette below the 16-color swatches shows foreground and
background colors currently used in your artwork. Reuse them with the same
left/right-click controls. Colors appear or disappear as you edit, undo,
paste, or open another image. Large color sets can be scrolled.

Use the **Attributes** buttons to change bold, blink/iCE, underline, or inverse
colors. Blink attributes are saved but are not animated in the editor.

## Selections and clipboard

| Task | How |
| --- | --- |
| Select an area | Choose **Selection > Rectangular selection**, then drag a rectangle. |
| Select everything / clear selection | **Selection > Select all / Deselect**. |
| Copy / cut | Select an area, then **Ctrl+C / Ctrl+X**, or use the **Edit** menu. |
| Paste | In Select mode, click the destination, then **Ctrl+V**. Deselect first if the destination is inside the existing selection. |
| Move an area | Drag inside the selection; release to complete the move. |
| Undo / redo | **Ctrl+Z / Ctrl+Y**, or the **Edit** menu. |

Copies within the editor preserve characters, colors, and attributes. Pasted
external text uses the brush style. Pasted areas must fit within the canvas.

Moving a selection clears its original area and overwrites the destination,
including blank cells. Moves, cuts, and pastes can each be undone as one action.

## Open and save artwork

Choose **File > Open...** to load an `.ans` file. Files without SAUCE dimensions
use the Width/Height in **Canvas**; set these before opening the file. Artwork
too large for those dimensions is rejected.

Choose **File > Encoding** before opening or saving. See
[Encoding and compatibility](#encoding-and-compatibility) for the choices.

Use **File > Save as...** to choose an output file, then **File > Save** for
subsequent changes.

### Save only a selection

Select an area, then choose **File > Save selection as...**.

This creates a separate, smaller ANSI file with the selected area's dimensions
and artwork credits. It does not change your full working image or replace
saving that image.

## SAUCE artwork credits

The **SAUCE metadata** section holds your title, author, group, and comments.
Enter one comment per line. If a loaded file has no SAUCE metadata, use
**Add SAUCE metadata** in this section.

SAUCE stores artwork credits and dimensions with the file, not as visible
characters on the canvas. Existing metadata is preserved when saving, with
your edits and the current image dimensions. Selection exports use the cropped
dimensions.

## Zoom and scrolling

Use the sidebar **zoom-out / zoom-in icons**, or click the **spyglass** and drag
its slider left/right. **100% is the center position**; the actual-size icon
resets it. The **View** menu offers the same commands.

| Shortcut | Action |
| --- | --- |
| Ctrl++ | Zoom in. |
| Ctrl+- | Zoom out. |
| Ctrl+0 | Return to 100%. |

Zoom ranges from **50% to 400%**, preserving proportions without changing the
saved artwork. Scrollbars appear whenever needed. At 100%, images taller than
50 rows always have a vertical scrollbar.

## Arrange the sidebar

Click the arrow on the **left** of a section heading to collapse or expand it.
Collapsed sections show only their shaded heading row.

Drag the handle at the **right edge** of a **Brush**, **Zoom**, **Colors**,
**Attributes**, or **SAUCE metadata** heading up or down, even when collapsed.
The highlighted line shows where the section will land. Release to finish,
or press **Esc** to cancel. Hold near the top or bottom of the sidebar to scroll
while dragging.

Your order and collapsed sections are saved automatically and restored when
ADraw restarts. New sections start expanded after your existing sections
without resetting their order. Sidebar preferences do not change your artwork
or its undo history.

## Lettering with TheDraw fonts

1. Unzip your font collection, then open **Edit > TheDraw fonts...** and choose
   **Import TDF files...**. You can import several `.tdf` files at once. ADraw
   copies them into `%APPDATA%\ADraw\data\fonts` and loads them automatically
   on future launches; you can move or remove the original files afterward.
2. Click a font in the scrollable **Imported fonts** list. Use **Search fonts**
   to filter by name. Enter text, adjust letter spacing, and check the live
   **Preview** beside the list. Browsing and previewing do not change your artwork.
3. Click the canvas to choose a destination, then select **Place text**.
   The lettering must fit; transparent cells leave existing artwork untouched.
4. Close the font window to return to ordinary drawing and typing.

Block and outline fonts use your brush colors; color fonts keep their own.
Text can span multiple lines, but only characters defined by the font are
available (ASCII, not arbitrary Unicode). Outline fonts currently use one
classic mixed single/double-line style.

Placement is undoable and selects the lettering, ready for
**Save selection as...**.

Imported fonts remain available across restarts. Collections with the same
filename are kept as separate copies when their contents differ; importing
the same unchanged file again reuses its copy. Files that cannot be imported
or restored produce an error message.

Already-placed lettering is saved as ordinary artwork and does not require
the font to view.
TDF font collections are not included with ADraw; the IBM VGA display font is
bundled separately.

## Encoding and compatibility

- **CP437** is for classic ANSI/BBS artwork; **UTF-8** supports Unicode text.
  Select the encoding under **File > Encoding** before opening or saving.
- ANSI files support 16-color, 256-color, and RGB colors. Your intended viewer
  must also support the colors and encoding you choose.
- Some CP437 graphical control characters cannot be saved safely in CP437 ANSI
  files. Choose UTF-8 when the editor reports this.
- Unicode characters outside CP437 use fallback fonts.

For source builds, testing, architecture, and credits, see the
[Developer guide](https://github.com/mikehodgson/adraw/blob/main/DEVELOPERS.md).
