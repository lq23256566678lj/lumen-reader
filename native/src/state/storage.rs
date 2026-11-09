use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::AppError;

pub const APP_DIR_NAME: &str = "LumenReader";

/// All on-disk locations used by the reader. Everything lives under
/// `dirs::data_dir()/LumenReader` as requested.
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub covers_dir: PathBuf,
    pub library_file: PathBuf,
    pub settings_file: PathBuf,
    pub cloud_file: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, AppError> {
        // `LUMEN_DATA_DIR` is useful for portable installs and for tests; by
        // default the path is exactly `dirs::data_dir()/LumenReader`.
        let data_dir = match std::env::var_os("LUMEN_DATA_DIR") {
            Some(directory) if !directory.is_empty() => PathBuf::from(directory),
            _ => dirs::data_dir()
                .ok_or(AppError::NoDataDir)?
                .join(APP_DIR_NAME),
        };
        let covers_dir = data_dir.join("covers");
        fs::create_dir_all(&covers_dir).map_err(|source| AppError::CreateDataDir {
            path: covers_dir.clone(),
            source,
        })?;
        Ok(Self {
            library_file: data_dir.join("library.json"),
            settings_file: data_dir.join("settings.json"),
            cloud_file: data_dir.join("cloud.json"),
            data_dir,
            covers_dir,
        })
    }

    pub fn cover_path(&self, cover_file: &str) -> PathBuf {
        self.covers_dir.join(cover_file)
    }

    /// Writes a cover as a downscaled PNG. A broken image is not fatal: the UI
    /// simply shows its generated placeholder.
    pub fn save_cover(&self, book_id: &str, bytes: &[u8]) -> Option<String> {
        let image = image::load_from_memory(bytes).ok()?;
        let thumbnail = image.thumbnail(360, 540);
        let file_name = format!("{book_id}.png");
        let path = self.covers_dir.join(&file_name);
        let mut cursor = std::io::Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut cursor, image::ImageFormat::Png)
            .ok()?;
        fs::write(&path, cursor.into_inner()).ok()?;
        Some(file_name)
    }

    pub fn remove_cover(&self, cover_file: &str) {
        let _ = fs::remove_file(self.cover_path(cover_file));
    }
}

pub fn load_json<T>(path: &Path) -> Result<T, AppError>
where
    T: DeserializeOwned + Default,
{
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(T::default()),
        Err(source) => {
            return Err(AppError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(text).map_err(AppError::Json)
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message(format!("无效的数据文件路径：{}", path.display())))?;
    fs::create_dir_all(parent).map_err(|source| AppError::CreateDataDir {
        path: parent.to_path_buf(),
        source,
    })?;

    let json = serde_json::to_vec_pretty(value)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| "data.json".to_owned(), |name| name.to_owned());
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));

    fs::write(&temp_path, json).map_err(|source| AppError::Write {
        path: temp_path.clone(),
        source,
    })?;

    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            // Windows cannot replace an existing file with rename. Remove the
            // old copy, then retry once.
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(remove_error) if remove_error.kind() == ErrorKind::NotFound => {}
                Err(remove_error) => {
                    let _ = fs::remove_file(&temp_path);
                    return Err(AppError::Write {
                        path: path.to_path_buf(),
                        source: remove_error,
                    });
                }
            }
            fs::rename(&temp_path, path).map_err(|source| AppError::Write {
                path: path.to_path_buf(),
                source,
            })?;
            log::debug!("replaced data file after rename fallback: {rename_error}");
            Ok(())
        }
    }
}

/// Content-based identity: size plus samples from the start and end. This is
/// stable when a book is moved or opened on another machine, which makes cloud
/// progress/bookmark merging useful without syncing the file itself.
pub fn fingerprint_file(path: &Path) -> Result<String, AppError> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::io::{Read, Seek, SeekFrom};

    let mut file = fs::File::open(path).map_err(|source| AppError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AppError::Read {
            path: path.to_path_buf(),
            source,
        })?
        .len();

    let mut hasher = DefaultHasher::new();
    length.hash(&mut hasher);

    let mut head = vec![0_u8; 64 * 1024];
    let read_head = file.read(&mut head).map_err(|source| AppError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    head.truncate(read_head);
    head.hash(&mut hasher);

    if length > 128 * 1024 {
        let tail_start = length.saturating_sub(64 * 1024);
        file.seek(SeekFrom::Start(tail_start))
            .map_err(|source| AppError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        let mut tail = vec![0_u8; 64 * 1024];
        let read_tail = file.read(&mut tail).map_err(|source| AppError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        tail.truncate(read_tail);
        tail.hash(&mut hasher);
    }

    Ok(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_content_based_not_path_based() {
        let unique = format!(
            "lumen-fingerprint-{}-{}",
            std::process::id(),
            crate::state::models::now_secs()
        );
        let directory = std::env::temp_dir().join(unique);
        let create = std::fs::create_dir_all(&directory);
        assert!(create.is_ok());
        let first = directory.join("first.epub");
        let second = directory.join("second.epub");
        let content = b"same content in two paths";
        assert!(std::fs::write(&first, content).is_ok());
        assert!(std::fs::write(&second, content).is_ok());

        let first_hash = fingerprint_file(&first);
        let second_hash = fingerprint_file(&second);
        assert_eq!(first_hash.ok(), second_hash.ok());

        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
        let _ = std::fs::remove_dir(directory);
    }
}
