# Changelog

All notable changes to ADraw are documented here. This changelog follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases use
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Changes after v0.1.0 stay under **Unreleased** until a release is explicitly
requested.

## [Unreleased]

### Added

- A 64 x 36 truecolor ANSI version of the application icon, with SAUCE metadata.
- An actual ADraw screenshot and a link to the example artwork in the README.

### Changed

- Windows installer and portable packages include the README screenshot,
  example artwork, and changelog so local documentation links work.
- Git preserves `.ans` files as raw bytes without text line-ending conversion.
- Contributor guidance requires changelog maintenance and explicit approval
  for version increases, release tags, and release publication.

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

[Unreleased]: https://github.com/mikehodgson/adraw/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/mikehodgson/adraw/releases/tag/v0.1.0
