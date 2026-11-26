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
  -Installer "$PWD\target\dist\adraw-0.3.0-windows-x64-setup.exe" `
  -PortableZip "$PWD\target\dist\adraw-0.3.0-windows-x64.zip"
```

The compiler helper downloads pinned Inno Setup 6.7.4, verifies its SHA-256
checksum and publisher signature, and installs it in portable mode without
system registration. Packaging builds the locked MSVC x64 release with the
static C runtime, then creates an installer, portable ZIP, and `SHA256SUMS.txt`
under `target\dist`. No runtime build tools are required by users.

The installer's optional `contextmenu` task registers per-user
`Software\Classes\SystemFileAssociations\.ans` and `.asc` `shell\ADraw`
verbs that run `adraw.exe -- "%1"`; `Test-Installer.ps1` checks they are
created and removed. Do not run `Test-Installer.ps1` on a machine where ADraw
is already installed.

The workspace release profile favors executable size (`opt-level = "z"`),
uses ThinLTO and one codegen unit, and strips symbols. These settings apply
to local release builds and CI packaging; debug builds are unchanged.
They trade optimization/build time and potentially some runtime speed for a
smaller executable. Panic unwinding, accessibility, bundled fonts, image
export, and TLS certificate verification remain enabled; no executable packer
is used. Compare sizes using the same MSVC target and static-CRT flags as the
packaging script, not a debug build.

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
git tag -a v0.3.0 -m "ADraw 0.3.0"
git push origin v0.3.0
```

Use the new version in place of `0.3.0`. The
[release workflow](.github/workflows/release.yml) runs for pushed version tags
with or without the `v` prefix, including prerelease tags. The tag must match
the Cargo version, or packaging fails rather than publishing a mislabeled
binary. It pins Rust 1.100.0 and official GitHub Actions commits, validates the
workspace on every supported native target, builds and smoke-tests the packages
described below, and publishes one GitHub release containing all platforms
with a combined checksum manifest and generated notes. Prerelease tags produce prereleases.
The publication job alone has repository write permission.

### Linux and macOS builds

Install Rust 1.100.0 and native build tools: Xcode Command Line Tools on macOS,
or a C compiler and `pkg-config` on Linux. On Ubuntu, the CI build and headless
launch-test dependencies can be installed with:

```sh
sudo apt-get install build-essential pkg-config libxkbcommon-dev libwayland-dev \
  libgl1-mesa-dev libegl1-mesa-dev libxkbcommon-x11-0 \
  libxcursor1 libxi6 libxrandr2 xvfb xauth dbus-x11
cargo test --locked --workspace
cargo check --locked --workspace
cargo run -p adraw
```

The [build workflow](.github/workflows/build.yml) runs on pushes to `main`,
pull requests, manual dispatch, and through the tag-triggered release workflow.
All targets run the workspace tests/checks and build a size-optimized release:

| Platform | Native runner | Rust target | Package |
| --- | --- | --- | --- |
| Windows x64 | Windows 2025 | `x86_64-pc-windows-msvc` | Installer and portable ZIP |
| Linux x64 | Ubuntu 22.04 | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Linux ARM64 | Ubuntu 22.04 ARM | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| macOS Intel | macOS 15 Intel | `x86_64-apple-darwin` | `.app` in `.tar.gz` |
| macOS Apple Silicon | macOS 15 ARM | `aarch64-apple-darwin` | `.app` in `.tar.gz` |

Build a native Unix package with Bash from the repository root, for example:

```sh
bash packaging/build-unix-release.sh x86_64-unknown-linux-gnu
# On Apple Silicon:
MACOSX_DEPLOYMENT_TARGET=11.0 bash packaging/build-unix-release.sh aarch64-apple-darwin
```

The script rejects mismatched host/target combinations and tags, creates
`target/dist/adraw-VERSION-PLATFORM.tar.gz`, and writes `SHA256SUMS.txt`.
Each archive contains documentation, license notices, and the example artwork.
Linux binaries dynamically link platform libraries; building on Ubuntu 22.04
sets the glibc baseline to 2.35, not universal compatibility with every distro.
See the [User guide](USER_GUIDE.md) for runtime libraries and portal setup.
macOS builds set the deployment target to 11.0, bundle an icon and versioned
Info.plist, and apply/verify an ad-hoc signature. They are not notarized or
Developer ID signed. No signing credentials are required.

Linux archives include `install.sh` (from `packaging/linux-install.sh`), which
installs per user under `~/.local` with a desktop entry, a `text/x-ansi` MIME
type for `*.ans` and `*.asc`, and an icon; `--uninstall` removes them. The
macOS Info.plist declares an imported `io.github.mikehodgson.adraw.ansi-art`
type and an Alternate-rank editor document type. `macos_open.rs` installs the
open-documents Apple Event handler before AppKit finishes launching, because
winit does not forward Finder open requests.

Package checks verify checksums, archive contents, executable permissions,
macOS architecture/plist/signature, and dynamic dependencies. Linux launches
the extracted executable under Xvfb with software OpenGL; macOS launches the
extracted app executable. Both must remain running for 15 seconds. CI also
runs `install.sh` in a temporary home directory and opens example artwork with
`open -a ADraw.app`. These
checks do not replace interactive testing of platform file dialogs, clipboard,
Wayland, accessibility, or Gatekeeper on downloaded apps.
CI artifacts are retained for inspection; only version-tag runs publish releases.

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

### Marketing website

The standalone [website](adraw-website/) uses Astro, Tailwind CSS, and Preline UI.
See its [README](adraw-website/README.md) for Node requirements, local preview,
release data, and static hosting. From `adraw-website`, run `npm ci`, `npm test`,
and `npm run build`. Website-only changes do not require rebuilding the Rust
workspace.

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
| [updates.rs](crates/ansi_editor/src/updates.rs) | Background official-release checks and dismissible download notifications. |
| [image_export.rs](crates/ansi_editor/src/image_export.rs) | Bounded native-resolution PNG/JPEG rendering and atomic export writes. |
| [cli.rs](crates/ansi_editor/src/cli.rs) | Command-line parsing: a bare file opens the editor; `--png`/`--jpeg` convert beside each source file without the GUI. |
| [macos_open.rs](crates/ansi_editor/src/macos_open.rs) | macOS Apple Event (`odoc`) handler that forwards Finder **Open With** requests to the editor. |
| [glyph.rs](crates/ansi_editor/src/glyph.rs) | IBM VGA bitmap rendering for all CP437 glyphs and shared cell geometry. |

The startup release check runs once on a worker thread, using the official
GitHub latest-release API without credentials. It has a 5-second connection
timeout, a 10-second overall timeout, a 256 KiB response limit, and no redirects.
Semantic version precedence ignores build metadata; only newer stable releases
are offered. Failures are logged without an editor error dialog. The notification
opens a fixed official release URL only on user request and does not install
updates. Tests use local HTTP fixtures rather than GitHub.

The About window uses the same `egui::Window` as the other tool windows,
including its built-in title bar and close control. It reuses the bundled
application icon and Cargo package version.
Its manual update button reuses the same checker and does not start overlapping
requests. Update status stays in the About dialog; automatic failures still
do not open an error notification. Canvas typing and global editing/zoom
shortcuts are gated while the About window is open.

Image export is desktop rendering, not a core file format. It uses the same
cell colors, embedded CP437 bitmap, and active fallback font definitions as
the editor, at 9 x 16 pixels per cell regardless of zoom. Unicode coverage
comes from egui's font atlas at native scale. Pixel allocation is limited to
32 megapixels and the glyph-mask cache is bounded. Export writes a complete
temporary image in the destination directory before replacing the chosen file;
errors must not change the document or its save path.

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
On Linux, storage uses `$XDG_DATA_HOME/adraw` (normally
`~/.local/share/adraw`); on macOS it uses `~/Library/Application Support/ADraw`.
The same native directory holds `app.ron` and the imported `fonts` folder.

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
  terminal emulator. Desktop opens use `ans_io::load_auto`: SAUCE width and height
  are handled independently, missing width defaults to 80, and missing height
  grows to the highest rendered row within the core cell limit. Explicit-size
  `ans_io::load` retains its fallback-dimension behavior for callers and tests.
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
