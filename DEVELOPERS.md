# Developer guide

This guide covers building and maintaining the software. For creating artwork,
start with [README.md](README.md) or see the detailed [User guide](USER_GUIDE.md).
Read [AGENTS.md](AGENTS.md) and any locally supplied task requirements before
changing application behavior.

## Build a Windows executable

Install:

- The [Rust toolchain](https://www.rust-lang.org/tools/install) for Windows,
  using the MSVC target.
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with the **Desktop development with C++** workload and a Windows SDK.

Run commands from the project root, containing [Cargo.toml](Cargo.toml):

```powershell
cargo build -p adraw --release
```

The executable is created at `target\release\adraw.exe`. Run that build
directly, or use the following during development:

```powershell
cargo run -p adraw
```

On Windows, close the running editor before rebuilding the same executable.
For an isolated debug build while another instance is running:

```powershell
cargo build -p adraw --target x86_64-pc-windows-msvc
```

That executable is at
`target\x86_64-pc-windows-msvc\debug\adraw.exe`.

### Build a Windows release

Run from the project root in PowerShell:

```powershell
$compiler = .\packaging\Get-InnoSetup.ps1 -Destination "$PWD\target\tools\innosetup"
.\packaging\Build-Release.ps1 -InnoCompiler $compiler
.\packaging\Test-Installer.ps1 `
  -Installer "$PWD\target\dist\adraw-0.1.0-windows-x64-setup.exe" `
  -PortableZip "$PWD\target\dist\adraw-0.1.0-windows-x64.zip"
```

The compiler helper downloads pinned Inno Setup 6.7.4, verifies its SHA-256
checksum and publisher signature, and installs it in portable mode without
system registration. Packaging builds the locked MSVC x64 release with the
static C runtime, then creates an installer, portable ZIP, and `SHA256SUMS.txt`
under `target\dist`. No runtime build tools are required by users.

The installer is per-user, uses a stable application ID for upgrades, and does
not remove Roaming application data or artwork. It does not close running
editors automatically. The smoke test verifies installed bytes against the ZIP,
checks version registration and artwork preservation, and uninstalls its
isolated test copy. It refuses to run if an ADraw installation is already
registered for the current user.
Release artifacts are currently unsigned.

### Publish a version

Version increases, version tags, and release publication require an explicit
user request. Committing or pushing ordinary changes to `main` must not bump
the version or create a release; record those changes under **Unreleased** in
[CHANGELOG.md](CHANGELOG.md).

For an authorized release, follow the changelog policy in
[AGENTS.md](AGENTS.md): move the completed Unreleased entries into a heading
for the new version and actual release date, retain the Unreleased section,
and update the comparison links.

Both crate versions inherit `workspace.package.version` in
[Cargo.toml](Cargo.toml). Update that version, run the validation commands below
to refresh and check the lockfile, and commit the changes to `main`. Then:

```powershell
gh auth setup-git
git push origin main
git tag -a v0.1.0 -m "ADraw 0.1.0"
git push origin v0.1.0
```

Use the new version in place of `0.1.0`. The
[release workflow](.github/workflows/release.yml) runs for pushed version tags
with or without the `v` prefix, including prerelease tags. The tag must match
the Cargo version, or packaging fails rather than publishing a mislabeled
binary. It pins Rust 1.100.0 and official GitHub Actions commits, validates the
workspace, builds and smoke-tests Windows artifacts, and publishes a GitHub
release with checksums and generated notes. Prerelease tags produce prereleases.
The publication job alone has repository write permission.

The application icon is bundled in
[adraw.ico](crates/ansi_editor/assets/adraw.ico).
[build.rs](crates/ansi_editor/build.rs) embeds its seven resolutions into Windows
executables using `winresource` and the Windows SDK resource compiler.
[adraw.png](crates/ansi_editor/assets/adraw.png) is the unchanged 256 x 256 PNG
extracted from that ICO, used for the running window/taskbar icon. When replacing
the icon, update both assets; a regression test checks they match.

## Validation

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo check --workspace
```

Use `cargo fmt --all` to apply formatting. Add focused regression tests for
changed behavior, and use targeted tests while iterating. Core tests cover
canvas/history operations, byte encodings, synthetic SAUCE fixtures, and ANSI
round trips. Editor tests exercise input routing, selections, menus, palettes,
zoom, and rendering through egui input/output.

### Optional TDF collection test

Use an extracted collection of `.tdf` files:

```powershell
$env:ADRAW_TDF_TEST_DIR = 'C:\path\to\extracted\fonts'
cargo test -p ansi_core external_collection_compatibility -- --ignored --nocapture
```

This ignored test loads collections, renders their defined characters, and
reports rejected files. Do not commit or redistribute third-party font
collections with the project.

## Workspace architecture

The [ansi_core crate](crates/ansi_core/) is GUI-independent. The
[adraw crate](crates/ansi_editor/) owns the native eframe/egui application.
Its source directory remains `crates/ansi_editor`; its Cargo package and
executable are named `adraw`. The application and Windows version metadata
use the product name **ADraw**.

| Area | Responsibility |
| --- | --- |
| [canvas.rs](crates/ansi_core/src/canvas.rs) | Character cells, 16/indexed/RGB colors, attributes, grid operations, atomic undo/redo. |
| [encoding.rs](crates/ansi_core/src/encoding.rs) | Explicit CP437 byte/glyph mapping and UTF-8 handling. |
| [sauce.rs](crates/ansi_core/src/sauce.rs) | SAUCE records, optional COMNT blocks, EOF handling, and metadata preservation. |
| [ans_io.rs](crates/ansi_core/src/ans_io.rs) | Byte-oriented ANSI parsing, serialization, and cropped exports. |
| [tdf.rs](crates/ansi_core/src/tdf.rs) | Bounded TheDraw font parsing and character-cell text rendering. |
| [main.rs](crates/ansi_editor/src/main.rs) | Menus, dialogs, palette/font pickers, editing input, selections, clipboard, and viewport zoom. |
| [font_library.rs](crates/ansi_editor/src/font_library.rs) | Persistent, validated TDF imports and startup font-library loading. |
| [glyph.rs](crates/ansi_editor/src/glyph.rs) | IBM VGA bitmap rendering for all CP437 glyphs and shared cell geometry. |

Keep file-format and document logic out of the GUI. Read ANSI files as raw
bytes; do not pass CP437 input through naive UTF-8 decoding. Strip SAUCE before
rendering, preserve original metadata fields, and report invalid or
unrepresentable data instead of silently clipping or substituting it.

Zoom is a view setting, not a document resize. Use the same scaled cell geometry
for painting, hit-testing, selection outlines, cursor navigation, and scrolling.
Batch selection moves and font placement into a single history action.

The startup viewport fits the default 80-column canvas and full metadata sidebar
with small padding below Dimensions. Its right margin reserves the solid
vertical scrollbar's width and inner spacing so taller 80-column canvases do not
introduce horizontal scrolling. Layout tests bound both margins.

Sidebar sections share one header, drag/drop implementation, and stable ID
scope. To add a section, register it in `SidebarSection::ALL`, provide a stable
`id` and `label`, and render its contents in `sidebar_section`; do not add a
separate fixed-position panel. Keep IDs unchanged across releases. Loading
preserves known saved IDs, removes duplicates/retired IDs, and appends missing
sections in registration order.

The sidebar order and collapsed section IDs are saved through eframe's
persistence storage under `sidebar_order_v1` and `sidebar_collapsed_v1` in
`%APPDATA%\ADraw\data\app.ron` on Windows. Missing collapse preferences and new
sections default to expanded. Shaded headers share a left collapse toggle and
right-aligned drag handle; collapsed bodies are not rendered, but remain
reorderable. Saving runs
automatically and on normal exit. Window geometry and egui memory persistence
are disabled to preserve existing startup behavior; artwork is not persisted
as a preference. Unavailable storage produces an editor notification, and
eframe logs filesystem storage failures.

TheDraw collections are copied into the `fonts` subdirectory of
`eframe::storage_dir("ADraw")` (`%APPDATA%\ADraw\data\fonts` on Windows).
Validate bounded raw bytes before saving; use a temporary file and
`persist_noclobber` to publish complete copies without overwriting existing
collections. Different contents with the same filename get numbered copies;
unchanged reimports reuse the existing copy. Font entries reference managed
copies, never the original import locations. Startup scans TDF files in
deterministic order, enforcing the library limit and reporting failed files
while retaining valid fonts. The searchable font list is virtualized, and its
adjacent preview does not modify the document until **Place text** is pressed.

The sidebar's shared used-color palette is derived from the current document,
including both foreground and background colors and bold/iCE brightness.
Identical displayed RGB colors share one swatch, retaining the first matching
ANSI/indexed/RGB representation. Swatches have deterministic RGB ordering and
virtualized, height-limited rows so large truecolor documents do not create a
button for every color on every frame. Left-click selects foreground and
right-click selects background without editing the document. Swatch IDs are
based on their RGB values, keeping interaction identity stable as colors change.

The 16-color and indexed palettes use the same left/right-click selection helper.
One FG/BG swatch pair displays the current brush colors; the RGB button and
current swatches open the foreground editor on left-click and background editor
on right-click. The popup remembers its target without changing the color or
its ANSI/indexed/RGB representation until the user edits it.

## Format and rendering notes

- ANSI support includes 16-color, indexed (`38;5` / `48;5`), and RGB
  (`38;2` / `48;2`) SGR, plus common CSI cursor commands; it is not a complete VT
  terminal emulator. Files without SAUCE use the configured canvas dimensions.
- Selection exports rebase cells to the origin, preserve metadata, and always
  include SAUCE with cropped dimensions. They must not change the full
  document's canvas, save path, dirty state, or history.
- CP437 graphical control-byte glyphs conflict with ANSI cursor controls.
  CP437 serialization rejects these glyphs; UTF-8 can represent them.
- All CP437 glyphs use the bundled IBM VGA 9x16 bitmap, with pixel-snapped,
  unfeathered meshes. Cells are 9 x 16 logical pixels at 100% zoom. The picker
  and TDF previews share this renderer; the brush input uses the matching
  TrueType font. Unicode outside CP437 uses the existing fallback fonts.
  Blink/iCE attributes are preserved, but blink is not animated.
- TDF imports support block, color, and classic outline fonts. Alternative
  outline styles are not implemented. Color fonts use DOS palette ordering and
  blink attributes; block/outline fonts use brush styling.
- Resource limits include one million canvas cells, 32 MiB per TDF file, and
  4096 fonts in the imported library. Font text is limited to 4096 characters and
  must fit the canvas before placement.

## Documentation and credits

- [README.md](README.md): quick start and main feature overview.
- [USER_GUIDE.md](USER_GUIDE.md): detailed editing and file workflows.
- [CHANGELOG.md](CHANGELOG.md): notable unreleased changes and release history.
- [AGENTS.md](AGENTS.md): contributor/agent rules.
- [Copilot instructions](.github/copilot-instructions.md): architecture guidance.

The bundled [IBM VGA 9x16 font](https://int10h.org/oldschool-pc-fonts/fontlist/font?ibm_vga_9x16)
is from **The Ultimate Oldschool PC Font Pack v2.2**, copyright 2016-2020
**VileR**, licensed under **CC BY-SA 4.0**. The full license is included in
[LICENSE-IBM-VGA.txt](crates/ansi_editor/assets/LICENSE-IBM-VGA.txt); retain it
and this attribution when redistributing the font assets. Font attribution
and the license link are also available in the CP437 picker. Rust source remains
MIT-licensed; the editor package declares both licenses to account for its
bundled font assets.

[ibm-vga-9x16.bin](crates/ansi_editor/assets/ibm-vga-9x16.bin) was converted from
`Bm437_IBM_VGA_9x16.FON` in the
[Windows font pack](https://int10h.org/oldschool-pc-fonts/download/oldschool_pc_font_pack_v2.2_win.zip):
256 CP437 glyphs, each containing 16 little-endian `u16` row masks. Bit 8 is
the leftmost pixel and bit 0 is the ninth column. The glyph shapes are
unchanged, including the font's ninth-column extensions. This converted
asset remains CC BY-SA 4.0.
[Px437_IBM_VGA_9x16.ttf](crates/ansi_editor/assets/Px437_IBM_VGA_9x16.ttf)
is bundled unchanged from the same archive for text-entry widgets.

The TDF parser follows
[Roy/SAC's format specification](https://www.roysac.com/blog/2014/04/thedraw-fonts-file-tdf-specifications/).
Material Symbols are provided by the MIT-licensed
[egui_material_icons](https://docs.rs/egui_material_icons/0.3.0/) crate; Google's
[Material icons](https://github.com/google/material-design-icons) are
Apache-2.0-licensed.
