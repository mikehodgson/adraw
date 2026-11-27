# Changelog

All notable changes to ADraw are documented here. This changelog follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases use
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Changes after the latest release stay under **Unreleased** until a release is explicitly
requested.

## [Unreleased]

### Added

- Choosing a foreground or background color while a selection is active
  recolors the selected cells, as a single undo step.
- Ctrl+click (Cmd+click on macOS) a color to fade the selection's foreground
  or background from the current color to the clicked one, along the
  direction the selection was dragged. This also works with Ctrl held inside
  the RGB picker. The gradient stays within 16 or 256
  colors when both endpoints do.
- While a selection is active, choose whether its gradient follows the drag,
  runs left to right, or runs top to bottom, and reverse it; the latest
  gradient is redrawn in place.
- An eyedropper tool picks up a cell's displayed foreground (left-click) or
  background (right-click) color from the artwork.
- A toggleable minimap shows the entire canvas and lets you navigate by
  clicking or dragging the outlined viewport without tinting the artwork.
- The default window width accommodates an 80-column canvas and the minimap.

### Changed

- Used colors lists only the colors you pick during the session, including
  gradient end colors, instead of every color in the artwork.
- Updated desktop dependencies and refreshed the Rust dependency lockfile.
- Switched desktop rendering from Glow to wgpu.
- Removed scroll-edge shading from the main canvas.
- Ignored the local VS Code workspace file.

### Fixed

- Resizing the minimap reuses its cached preview instead of rebuilding it
  on every width change.
- Drawing with the minimap visible refreshes only the changed preview region,
  avoiding a full preview redraw for every painted cell.

## [0.4.0] - 2026-11-26

### Added

- Astro marketing website with responsive screenshots, platform downloads,
  live official-release information, and a searchable, task-focused user guide.
- Command-line conversion of ANSI files to PNG or JPEG (`adraw --png FILE.ans`
  or `adraw --jpeg FILE.ans`), writing each image beside its source with the
  same name and a `.png` or `.jpg` extension. `adraw FILE.ans` opens the file
  in the editor.
- **Open with ADraw** for `.ans` and `.asc` files: an optional Windows
  installer context-menu entry, a Linux `install.sh` that registers a desktop
  entry and file type, and macOS Finder **Open With** support. The Open dialog
  also lists `.asc` files.

## [0.3.0] - 2026-11-23

### Added

- Linux x64/ARM64 and macOS Intel/Apple Silicon binary packages, with native
  CI tests and package launch checks. Version-tag releases include all platforms
  and combined SHA-256 checksums.

### Changed

- Keyboard shortcuts and their labels use Command on macOS and Ctrl on
  Windows/Linux, including Command+Shift+Z for macOS redo; application-data
  messages and bundled-font startup are portable.
- Smaller release executables through size-focused optimization, ThinLTO,
  a single codegen unit, and symbol stripping, without removing application features.

## [0.2.0] - 2026-11-23

### Added

- Added an about window with manual update check with progress and results.
- PNG and JPEG exports in the File menu render the active selection or the
  full artwork at native character-cell resolution.
- Startup check for newer official GitHub releases.

### Changed

- Windows installer and portable packages include the README screenshot,
  example artwork, and changelog so local documentation links work.

### Fixed

- Opening artwork resets canvas dimensions to the file's SAUCE dimensions,
  or 80 columns and an inferred height when dimensions are missing. New images
  always reset to the default 80 x 25 canvas instead of reusing the previous size.

## [0.1.0] - 2026-11-22

### Added

- Initial Windows desktop release with custom-sized canvases, character
  brushes, direct typing, selections, styled copy/paste, and undo/redo.
- CP437 and UTF-8 workflows with a bundled IBM VGA 9x16 display font.
- ANSI loading and saving with 16-color, 256-color, and 24-bit RGB SGR support,
  common cursor commands, and SAUCE artwork metadata.
- Shared foreground/background palettes, dynamically used artwork colors,
  proportional zoom, and a persistent reorderable, collapsible sidebar.
- A persistent TheDraw font library with searchable selection, live previews,
  and undoable text placement.
- A user guide, per-user Windows installer, portable ZIP, checksums, and
  automatic GitHub releases from explicitly pushed version tags.

[Unreleased]: https://github.com/mikehodgson/adraw/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/mikehodgson/adraw/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/mikehodgson/adraw/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/mikehodgson/adraw/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mikehodgson/adraw/releases/tag/v0.1.0
