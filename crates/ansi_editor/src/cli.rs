use crate::image_export::{self, Format};
use ansi_core::{ans_io, encoding::Encoding};
use eframe::egui::FontDefinitions;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const USAGE: &str = "\
Usage: adraw [--utf8] [FILE]
       adraw (--png | --jpeg) [--utf8] FILE...

Without --png or --jpeg, open the editor, optionally loading FILE.

With --png or --jpeg, convert ANSI art to rendered images without opening the
editor. Each image is written beside its source with the same name and a .png
or .jpg extension, replacing an existing image of that name.

Options:
  --png       Convert to PNG
  --jpeg      Convert to JPEG (alias: --jpg)
  --utf8      Read input as UTF-8 instead of CP437
  -h, --help  Show this help";

#[derive(Debug, PartialEq)]
pub enum Command {
    Edit {
        path: Option<PathBuf>,
        encoding: Encoding,
    },
    Help,
    Convert {
        format: Format,
        encoding: Encoding,
        inputs: Vec<PathBuf>,
    },
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut format = None;
    let mut encoding = Encoding::Cp437;
    let mut inputs = Vec::new();
    let mut options = true;
    for arg in args {
        if options {
            match arg.to_str() {
                Some("-h" | "--help") => return Ok(Command::Help),
                Some("--") => {
                    options = false;
                    continue;
                }
                Some(flag @ ("--png" | "--jpeg" | "--jpg")) => {
                    let requested = if flag == "--png" {
                        Format::Png
                    } else {
                        Format::Jpeg
                    };
                    if format.is_some_and(|format| format != requested) {
                        return Err("Choose only one of --png or --jpeg.".into());
                    }
                    format = Some(requested);
                    continue;
                }
                Some("--utf8") => {
                    encoding = Encoding::Utf8;
                    continue;
                }
                Some(flag) if flag.starts_with('-') && flag.len() > 1 => {
                    return Err(format!("Unknown option {flag}."));
                }
                _ => {}
            }
        }
        inputs.push(PathBuf::from(arg));
    }
    let Some(format) = format else {
        if inputs.len() > 1 {
            return Err("Open one file at a time, or add --png or --jpeg to convert.".into());
        }
        return Ok(Command::Edit {
            path: inputs.pop(),
            encoding,
        });
    };
    if inputs.is_empty() {
        return Err("Name at least one ANSI file to convert.".into());
    }
    Ok(Command::Convert {
        format,
        encoding,
        inputs,
    })
}

pub fn output_path(input: &Path, format: Format) -> Result<PathBuf, String> {
    if input.file_stem().is_none() {
        return Err(format!("{} is not a file name.", input.display()));
    }
    let output = input.with_extension(format.extension());
    if output == input {
        return Err(format!(
            "{} already has the .{} extension; converting it would overwrite the source.",
            input.display(),
            format.extension()
        ));
    }
    Ok(output)
}

pub fn convert(
    input: &Path,
    format: Format,
    encoding: Encoding,
    definitions: FontDefinitions,
) -> Result<PathBuf, String> {
    let output = output_path(input, format)?;
    let bytes = std::fs::read(input).map_err(|error| format!("Could not read file: {error}"))?;
    let doc = ans_io::load_auto(&bytes, encoding)?;
    let image = image_export::render(&doc, None, definitions)?;
    image_export::save(&output, image, format)?;
    Ok(output)
}

/// Runs a non-editor command, returning the process exit code.
pub fn run(command: Result<Command, String>, definitions: FontDefinitions) -> i32 {
    let (format, encoding, inputs) = match command {
        Ok(Command::Help) => {
            println!("{USAGE}");
            return 0;
        }
        Ok(Command::Convert {
            format,
            encoding,
            inputs,
        }) => (format, encoding, inputs),
        Ok(Command::Edit { .. }) => {
            eprintln!("adraw: Add --png or --jpeg to convert files.\n\n{USAGE}");
            return 2;
        }
        Err(error) => {
            eprintln!("adraw: {error}\n\n{USAGE}");
            return 2;
        }
    };
    let mut failed = false;
    for input in inputs {
        match convert(&input, format, encoding, definitions.clone()) {
            Ok(output) => println!("{} -> {}", input.display(), output.display()),
            Err(error) => {
                eprintln!("adraw: {}: {error}", input.display());
                failed = true;
            }
        }
    }
    i32::from(failed)
}

/// Windows GUI executables have no console; reuse the launching terminal's.
#[cfg(windows)]
pub fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: AttachConsole has no memory-safety preconditions; failure leaves
    // the process without a console, matching the previous behavior.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_formats_encodings_and_rejects_ambiguous_arguments() {
        assert_eq!(
            parse(args(&[])).unwrap(),
            Command::Edit {
                path: None,
                encoding: Encoding::Cp437,
            }
        );
        assert_eq!(
            parse(args(&["--", "-my screen.asc"])).unwrap(),
            Command::Edit {
                path: Some("-my screen.asc".into()),
                encoding: Encoding::Cp437,
            }
        );
        assert!(parse(args(&["a.ans", "b.ans"])).is_err());
        assert_eq!(
            parse(args(&["--png", "my-screen.ans"])).unwrap(),
            Command::Convert {
                format: Format::Png,
                encoding: Encoding::Cp437,
                inputs: vec!["my-screen.ans".into()],
            }
        );
        assert_eq!(
            parse(args(&["--jpg", "--utf8", "a.ans", "--", "--b.ans"])).unwrap(),
            Command::Convert {
                format: Format::Jpeg,
                encoding: Encoding::Utf8,
                inputs: vec!["a.ans".into(), "--b.ans".into()],
            }
        );
        assert_eq!(parse(args(&["a.ans", "--help"])).unwrap(), Command::Help);
        assert!(parse(args(&["--png", "--jpeg", "a.ans"])).is_err());
        assert!(parse(args(&["--gif", "a.ans"])).is_err());
        assert!(parse(args(&["--png"])).is_err());
    }

    #[test]
    fn output_keeps_the_name_and_only_changes_the_extension() {
        let dir = Path::new("art").join("my-screen.ans");
        assert_eq!(
            output_path(&dir, Format::Png).unwrap(),
            Path::new("art").join("my-screen.png")
        );
        assert_eq!(
            output_path(&dir, Format::Jpeg).unwrap(),
            Path::new("art").join("my-screen.jpg")
        );
        assert_eq!(
            output_path(Path::new("v1.2.ans"), Format::Png).unwrap(),
            Path::new("v1.2.png")
        );
        assert_eq!(
            output_path(Path::new("noext"), Format::Png).unwrap(),
            Path::new("noext.png")
        );
        assert!(output_path(Path::new("shot.png"), Format::Png).is_err());
        assert!(output_path(Path::new(".."), Format::Png).is_err());
    }

    #[test]
    fn converts_files_beside_their_sources_and_reports_failures() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("my-screen.ans");
        std::fs::write(&input, b"\x1b[31mA\xdb").unwrap();
        for format in [Format::Png, Format::Jpeg] {
            let output =
                convert(&input, format, Encoding::Cp437, FontDefinitions::default()).unwrap();
            assert_eq!(
                output,
                directory
                    .path()
                    .join(format!("my-screen.{}", format.extension()))
            );
            let decoded = image::open(&output).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (80 * 9, 16));
        }
        let missing = directory.path().join("missing.ans");
        assert_eq!(
            run(
                parse([OsString::from("--png"), input.into(), missing.into()]),
                FontDefinitions::default()
            ),
            1
        );
        assert!(!directory.path().join("missing.png").exists());
        assert_eq!(
            run(parse(args(&["--bogus"])), FontDefinitions::default()),
            2
        );
    }
}
