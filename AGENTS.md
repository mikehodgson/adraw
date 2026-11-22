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

