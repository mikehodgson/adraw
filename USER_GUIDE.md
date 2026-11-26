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

Linux and macOS packages are available from 0.3.0:

- **Linux:** choose `linux-x64` or `linux-arm64`, extract the `.tar.gz`,
  and run `./adraw` from the extracted directory. Use Ubuntu 22.04 or a
  compatible newer desktop with glibc 2.35+, OpenGL, and X11 or Wayland.
  File dialogs require a running D-Bus session and `xdg-desktop-portal` with
  a desktop backend (such as `xdg-desktop-portal-gtk` or the KDE equivalent).
  On Ubuntu, install missing runtime packages with
  `sudo apt install libgl1 libegl1 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libxcursor1 libxi6 libxrandr2 xdg-desktop-portal xdg-desktop-portal-gtk`.
  These are not static binaries or AppImages.
- **macOS:** choose `macos-arm64` for Apple Silicon or `macos-x64` for Intel,
  extract the `.tar.gz`, and move `ADraw.app` to Applications.
  macOS 11 or newer is required. Apps have an ad-hoc signature, not a
  Developer ID signature or Apple notarization. After attempting to open a
  trusted official download, use **System Settings > Privacy & Security >
  Open Anyway** if macOS blocks it. Do not disable Gatekeeper globally.

On macOS, use **Command** instead of **Ctrl** for the shortcuts below.
Redo is **Command+Shift+Z** (**Command+Y** is also supported).
The menus and tooltips show your platform's shortcut keys.
The bundled CP437 font works on every platform; other Unicode glyphs use
the available fallback fonts and can differ between systems.

At startup, ADraw checks the official GitHub releases in the background.
If a newer stable version is available, choose **Download update** to open
the release page in your browser, or **Not now** (or close the notification)
to keep editing. Dismissal lasts until the next launch. No update is downloaded
or installed automatically. Offline, timed-out, or inaccessible checks do not
show an error or prevent you from using ADraw.

Choose **About** in the top menu to see the application logo, current version,
and a link to the GitHub repository. **Check for updates** runs the same
background check on demand and reports whether a newer release is available,
you are up to date, or the check is unavailable. If a check is already running,
wait for it to finish before trying again. A download link opens the official
release page; nothing is installed automatically. About uses the same movable,
resizable, collapsible window as the other tools. Close it with its title-bar
**X**; clicking outside does not dismiss it.

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
New images always reset to these defaults, regardless of the previous image.
For a custom size, create a new image, then set **Width** and **Height** in the
**Canvas** menu and choose **Resize canvas**.

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

Choose **File > Open...** to load an `.ans` file. Opening replaces the canvas
dimensions and the **Canvas** menu's Width and Height with those of the file.
SAUCE dimensions take precedence. If width is missing, ADraw assumes **80**
columns; if height is missing, it is inferred from the artwork's rendered rows.
The previous image's dimensions are never used. Trailing line breaks do not
add empty rows, and an empty file opens as an 80 x 1 canvas.

Choose **File > Encoding** before opening or saving. See
[Encoding and compatibility](#encoding-and-compatibility) for the choices.

Use **File > Save as...** to choose an output file, then **File > Save** for
subsequent changes.

### Save only a selection

Select an area, then choose **File > Save selection as...**.

This creates a separate, smaller ANSI file with the selected area's dimensions
and artwork credits. It does not change your full working image or replace
saving that image.

### Export a rendered image

Choose **File > Export PNG...** or **File > Export JPEG...** and choose a
filename. With an active selection, only that rectangle is exported;
otherwise the full canvas is rendered.

Images use the display font and colors at native **9 x 16 pixels per cell**,
independent of zoom or window size. Backgrounds, bold/bright colors, inverse,
underline, and iCE colors are included; blink is captured as steady text,
as in the editor. Cursor, selection outlines, sidebar, and SAUCE metadata
are not part of the image.

PNG preserves pixels exactly; JPEG uses quality 90 and may soften fine details.
Image exports do not change your ANSI save path, artwork, selection, or undo
history, and do not mark unsaved artwork as saved. Keep the ANSI file for later
editing. Exports are limited to 32 megapixels; select a smaller area if needed.
Characters missing from the available display fonts produce an actionable error
rather than a substituted glyph.

### Convert from the command line

`adraw my-screen.ans` opens the file in the editor. Add `--png` or `--jpeg`
to convert ANSI files to images without opening the editor:

```text
adraw --png my-screen.ans        # writes my-screen.png
adraw --jpeg art/*.ans           # writes art/<name>.jpg for each file
```

Each image is written beside its source with the same name, changing only the
extension to `.png` or `.jpg` and replacing an existing image of that name.
Files are read as CP437; add `--utf8` for UTF-8 artwork. Put `--` before file
names that start with `-`.
The full canvas is rendered exactly as **File > Export** does. Each converted
file is listed; unreadable or invalid files are reported and the command exits
with a non-zero status after converting the rest. Run `adraw --help` for all
options. On Windows, PowerShell and Command Prompt do not wait for ADraw to
finish; use `Start-Process adraw -ArgumentList '--png','my-screen.ans' -Wait`
in scripts.

### Open from your file manager

ADraw can appear as an **Open with ADraw** choice for `.ans` and `.asc` files.
It does not replace your default program for these files.

- **Windows:** the installer adds **Open with ADraw** to the Explorer context
  menu (the **Add "Open with ADraw" to the context menu** option, on by
  default). On Windows 11, choose **Show more options** to see it. Uninstalling
  removes it. The portable ZIP does not register the menu entry.
- **Linux:** from the extracted directory, run `./install.sh`. It installs
  ADraw for your user in `~/.local/bin`, adds a desktop menu entry, and offers
  ADraw under **Open With** for `.ans` and `.asc` files. Add `~/.local/bin` to
  your `PATH` to run `adraw` from a terminal. Run `./install.sh --uninstall`
  to remove it.
- **macOS:** after moving `ADraw.app` to Applications, choose
  **Open With > ADraw** from a file's Finder context menu.

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
   copies them into ADraw's application data folder and loads them automatically
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

Fonts are stored under `%APPDATA%\ADraw\data\fonts` on Windows,
`~/.local/share/adraw/fonts` on Linux (or `$XDG_DATA_HOME/adraw/fonts`),
and `~/Library/Application Support/ADraw/fonts` on macOS.

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
