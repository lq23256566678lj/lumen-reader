use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::error::BookError;
use crate::parser;
use crate::state::models::{BookFormat, BookRecord, now_secs};
use crate::state::storage::{AppPaths, fingerprint_file};

#[derive(Debug)]
pub enum ImportEvent {
    Started { total: usize },
    Imported(BookRecord),
    Failed { path: String, error: String },
    Finished { imported: usize, failed: usize },
}

pub struct ImportTask {
    receiver: Receiver<ImportEvent>,
    pub total: usize,
    pub done: usize,
    pub imported: usize,
    pub failed: usize,
    pub finished: bool,
}

impl ImportTask {
    pub fn spawn(paths: Vec<PathBuf>, app_paths: AppPaths) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let files = collect_book_files(&paths);
            let total = files.len();
            let _ = sender.send(ImportEvent::Started { total });
            let mut imported = 0;
            let mut failed = 0;

            for path in files {
                match import_one(&path, &app_paths) {
                    Ok(record) => {
                        imported += 1;
                        let _ = sender.send(ImportEvent::Imported(record));
                    }
                    Err(error) => {
                        failed += 1;
                        let _ = sender.send(ImportEvent::Failed {
                            path: path.to_string_lossy().into_owned(),
                            error: error.to_string(),
                        });
                    }
                }
            }

            let _ = sender.send(ImportEvent::Finished { imported, failed });
            // Keep the channel alive until every event is consumed.
            std::mem::drop(sender);
        });

        Self {
            receiver,
            total: 0,
            done: 0,
            imported: 0,
            failed: 0,
            finished: false,
        }
    }

    pub fn try_recv(&mut self) -> Option<ImportEvent> {
        match self.receiver.try_recv() {
            Ok(event) => {
                match &event {
                    ImportEvent::Started { total } => self.total = *total,
                    ImportEvent::Imported(_) => {
                        self.done += 1;
                        self.imported += 1;
                    }
                    ImportEvent::Failed { .. } => {
                        self.done += 1;
                        self.failed += 1;
                    }
                    ImportEvent::Finished { .. } => self.finished = true,
                }
                Some(event)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                None
            }
        }
    }

    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

fn import_one(path: &Path, app_paths: &AppPaths) -> Result<BookRecord, BookError> {
    let source = parser::open_book(path)?;
    let metadata = source.metadata().clone();
    let chapter_count = source.chapter_count();
    let cover = parser::extract_cover(source.as_ref());
    let file_size = std::fs::metadata(path).map_or(0, |meta| meta.len());
    let id = fingerprint_file(path).map_err(|error| BookError::Parse(error.to_string()))?;
    let cover_file = cover
        .as_deref()
        .and_then(|bytes| app_paths.save_cover(&id, bytes));

    Ok(BookRecord {
        id,
        title: metadata.title,
        author: metadata.author,
        collection_id: None,
        format: metadata.format,
        path: Some(path.to_string_lossy().into_owned()),
        file_size,
        cover_file,
        chapter_count,
        progress: Default::default(),
        added_at: now_secs(),
        last_opened_at: None,
    })
}

/// Expands files and directories into a deduplicated list of supported books.
pub fn collect_book_files(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<PathBuf> = inputs.iter().rev().cloned().collect();

    while let Some(path) = stack.pop() {
        if path.is_dir() {
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(error) => {
                    log::warn!("cannot read directory {}: {error}", path.display());
                    continue;
                }
            };
            let mut children: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect();
            children.sort();
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        } else if path.is_file()
            && BookFormat::from_path(&path).is_some()
            && seen.insert(path.clone())
        {
            files.push(path);
        }
    }

    files.sort();
    files
}
