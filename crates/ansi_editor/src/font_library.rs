use ansi_core::tdf;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub struct ImportedFont {
    pub font: tdf::Font,
    pub source: PathBuf,
}

#[derive(Default)]
pub struct FontLibrary {
    pub fonts: Vec<ImportedFont>,
    directory: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ImportResult {
    pub first: usize,
    pub added: usize,
}

fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(tdf::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

impl FontLibrary {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self {
            directory,
            fonts: Vec::new(),
        }
    }

    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    fn append(&mut self, source: PathBuf, fonts: Vec<tdf::Font>) -> ImportResult {
        let result = ImportResult {
            first: self.fonts.len(),
            added: fonts.len(),
        };
        self.fonts
            .extend(fonts.into_iter().map(|font| ImportedFont {
                font,
                source: source.clone(),
            }));
        result
    }

    fn check_capacity(&self, added: usize) -> Result<(), String> {
        if self.fonts.len() + added > tdf::MAX_FONTS {
            return Err("Imported font library would exceed 4096 fonts".into());
        }
        Ok(())
    }

    pub fn load(&mut self) -> Vec<String> {
        self.fonts.clear();
        let Some(directory) = self.directory.clone() else {
            return vec![
                "Cannot locate ADraw's application data folder. Check access to your user profile."
                    .into(),
            ];
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
            Err(error) => return vec![format!("{}: {error}", directory.display())],
        };
        let mut errors = Vec::new();
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => match entry.file_type() {
                    Ok(kind) if kind.is_file() => {
                        let path = entry.path();
                        if path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("tdf"))
                        {
                            paths.push(path);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => errors.push(format!("{}: {error}", entry.path().display())),
                },
                Err(error) => errors.push(format!("{}: {error}", directory.display())),
            }
        }
        paths.sort();
        for path in paths {
            let result = read_bytes(&path)
                .map_err(|error| format!("Could not read font file: {error}"))
                .and_then(|bytes| tdf::load(&bytes))
                .and_then(|fonts| {
                    self.check_capacity(fonts.len())?;
                    Ok(fonts)
                });
            match result {
                Ok(fonts) => {
                    self.append(path, fonts);
                }
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            }
        }
        errors
    }

    pub fn import(&mut self, source: &Path) -> Result<ImportResult, String> {
        let bytes =
            read_bytes(source).map_err(|error| format!("Could not read font file: {error}"))?;
        let fonts = tdf::load(&bytes)?;
        let directory = self
            .directory
            .clone()
            .ok_or("Cannot locate ADraw's application data folder; fonts cannot be saved")?;
        let stem = source.file_stem().ok_or("The font file has no filename")?;
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;

        for suffix in 1..=tdf::MAX_FONTS + 1 {
            let mut name = stem.to_os_string();
            if suffix > 1 {
                name.push(format!(" ({suffix})"));
            }
            name.push(".tdf");
            let destination = directory.join(name);
            match read_bytes(&destination) {
                Ok(existing) if existing == bytes => {
                    if let Some(first) = self
                        .fonts
                        .iter()
                        .position(|font| font.source == destination)
                    {
                        return Ok(ImportResult { first, added: 0 });
                    }
                    self.check_capacity(fonts.len())?;
                    return Ok(self.append(destination, fonts));
                }
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("Could not read {}: {error}", destination.display()));
                }
            }
            self.check_capacity(fonts.len())?;
            // Publish a complete file without overwriting another import, even in another instance.
            let mut copy = tempfile::Builder::new()
                .prefix(".tdf-import-")
                .tempfile_in(&directory)
                .map_err(|error| format!("Could not save imported font: {error}"))?;
            copy.write_all(&bytes)
                .and_then(|_| copy.as_file().sync_all())
                .map_err(|error| format!("Could not save imported font: {error}"))?;
            match copy.persist_noclobber(&destination) {
                Ok(_) => return Ok(self.append(destination, fonts)),
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "Could not save {}: {}",
                        destination.display(),
                        error.error
                    ));
                }
            }
        }
        Err("Could not find an unused filename for this font collection".into())
    }
}
