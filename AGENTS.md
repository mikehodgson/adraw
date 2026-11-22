# Agent guidance

- Read the requirements provided for the task before changing behavior.
  Locally supplied requirements are not included in the repository.
- The product name is ADraw. The desktop Cargo package and executable are
  `adraw`, with source in `crates/ansi_editor`.
- Keep [README.md](README.md) concise and artist-facing: getting started and
  a basic feature overview. Put detailed usage in [USER_GUIDE.md](USER_GUIDE.md),
  and build, architecture, and testing instructions in [DEVELOPERS.md](DEVELOPERS.md).
- `crates/ansi_core` owns the document model and byte-oriented file formats;
  it must not depend on the GUI. `crates/ansi_editor` owns desktop interaction.
- Register new sidebar sections in `SidebarSection::ALL` and the shared
  section renderer. Keep section IDs stable; preserve saved ordering and
  append newly registered sections without resetting existing preferences.
- Keep CP437 and UTF-8 choices explicit. Preserve original SAUCE fields when
  modifying metadata; never display metadata as artwork.
- Reject malformed or unrepresentable data with actionable errors rather than
  silently corrupting files. Bound dimensions and parser resource use.
- Add focused tests for file formats and document behavior; run `cargo test
  --workspace` and `cargo check --workspace` after relevant changes.
- Preserve existing user files and avoid unrelated changes.

## Changelog and release policy

- Read [CHANGELOG.md](CHANGELOG.md) before making changes. Add a concise,
  user- or contributor-facing entry under **Unreleased** for each notable
  change in the same commit as the change. Do not use a raw commit log.
- Follow Keep a Changelog: group entries under Added, Changed, Deprecated,
  Removed, Fixed, or Security; omit empty categories, merge related entries,
  and keep the newest releases first. Never put unreleased work under an
  already published version.
- Do not increase ADraw's release version, create or push version tags, or
  publish releases unless the user explicitly requests them. A request to
  commit or push to `main` is not release authorization. Normal changes remain **Unreleased**
  and leave the workspace release version, corresponding local-package
  entries in `Cargo.lock`, and executable version metadata unchanged.
- Only for an explicitly requested release, choose the authorized semantic
  version, move the completed Unreleased entries into a version heading with
  the actual release date (`YYYY-MM-DD`), retain an Unreleased section, and
  update the comparison links and version metadata together. Follow the
  release procedure in [DEVELOPERS.md](DEVELOPERS.md).

