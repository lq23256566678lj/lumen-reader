use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cloud::config::WebDavConfig;
use crate::cloud::webdav::WebDavClient;
use crate::error::CloudError;
use crate::state::models::{
    BookFormat, BookRecord, Bookmark, Collection, LibraryData, ReadingProgress, now_secs,
};

pub const CLOUD_SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CloudBook {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub collection_id: Option<String>,
    pub format: BookFormat,
    pub chapter_count: usize,
    #[serde(default)]
    pub progress: ReadingProgress,
    #[serde(default)]
    pub added_at: u64,
    #[serde(default)]
    pub last_opened_at: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CloudSnapshot {
    #[serde(default = "cloud_schema")]
    pub schema: u32,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub books: Vec<CloudBook>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub collections: Vec<Collection>,
}

fn cloud_schema() -> u32 {
    CLOUD_SCHEMA
}

impl Default for CloudSnapshot {
    fn default() -> Self {
        Self {
            schema: CLOUD_SCHEMA,
            device_id: String::new(),
            updated_at: 0,
            books: Vec::new(),
            bookmarks: Vec::new(),
            collections: Vec::new(),
        }
    }
}

impl CloudSnapshot {
    pub fn from_library(library: &LibraryData, device_id: &str) -> Self {
        Self {
            schema: CLOUD_SCHEMA,
            device_id: device_id.to_owned(),
            updated_at: now_secs(),
            books: library
                .books
                .iter()
                .map(|book| CloudBook {
                    id: book.id.clone(),
                    title: book.title.clone(),
                    author: book.author.clone(),
                    collection_id: book.collection_id.clone(),
                    format: book.format,
                    chapter_count: book.chapter_count,
                    progress: book.progress.clone(),
                    added_at: book.added_at,
                    last_opened_at: book.last_opened_at,
                })
                .collect(),
            bookmarks: library.bookmarks.clone(),
            collections: library.collections.clone(),
        }
    }

    pub fn apply_to_library(&self, library: &mut LibraryData) {
        for cloud_book in &self.books {
            if let Some(local) = library.book_mut(&cloud_book.id) {
                if cloud_book.progress.updated_at > local.progress.updated_at {
                    local.progress = cloud_book.progress.clone();
                    local.collection_id = cloud_book.collection_id.clone();
                } else if local.collection_id.is_none() {
                    local.collection_id = cloud_book.collection_id.clone();
                }
                if local.title.trim().is_empty() {
                    local.title = cloud_book.title.clone();
                }
                if local.author.trim().is_empty() {
                    local.author = cloud_book.author.clone();
                }
                if cloud_book.chapter_count > local.chapter_count {
                    local.chapter_count = cloud_book.chapter_count;
                }
                if cloud_book.last_opened_at > local.last_opened_at {
                    local.last_opened_at = cloud_book.last_opened_at;
                }
            } else {
                library.books.push(BookRecord {
                    id: cloud_book.id.clone(),
                    title: cloud_book.title.clone(),
                    author: cloud_book.author.clone(),
                    collection_id: cloud_book.collection_id.clone(),
                    format: cloud_book.format,
                    path: None,
                    file_size: 0,
                    cover_file: None,
                    chapter_count: cloud_book.chapter_count,
                    progress: cloud_book.progress.clone(),
                    added_at: cloud_book.added_at,
                    last_opened_at: cloud_book.last_opened_at,
                });
            }
        }

        library.bookmarks = self.bookmarks.clone();
        library.collections = self.collections.clone();
        library.clear_deleted_collection_links();
        library.version = 2;
    }

    pub fn validate(&self) -> Result<(), CloudError> {
        if self.schema != CLOUD_SCHEMA {
            return Err(CloudError::InvalidData(format!(
                "不支持的云同步数据结构版本 {}（期望 {}）",
                self.schema, CLOUD_SCHEMA
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SyncReport {
    pub snapshot: CloudSnapshot,
    pub remote_existed: bool,
    pub books: usize,
    pub bookmarks: usize,
    pub uploaded_at: u64,
}

pub async fn sync_now(
    config: &WebDavConfig,
    local: CloudSnapshot,
) -> Result<SyncReport, CloudError> {
    let client = WebDavClient::new(config)?;
    client.ensure_directory().await?;
    let remote = client.get_snapshot().await?;
    if let Some(remote) = &remote {
        remote.validate()?;
    }
    let remote_existed = remote.is_some();
    let merged = merge_snapshots(local, remote);
    client.put_snapshot(&merged).await?;
    Ok(SyncReport {
        books: merged.books.len(),
        bookmarks: merged.bookmarks.len(),
        uploaded_at: merged.updated_at,
        remote_existed,
        snapshot: merged,
    })
}

pub async fn test_connection(config: &WebDavConfig) -> Result<String, CloudError> {
    WebDavClient::new(config)?.test_connection().await
}

pub fn merge_snapshots(local: CloudSnapshot, remote: Option<CloudSnapshot>) -> CloudSnapshot {
    let Some(remote) = remote else {
        return CloudSnapshot {
            schema: CLOUD_SCHEMA,
            device_id: local.device_id,
            updated_at: now_secs(),
            books: local.books,
            bookmarks: local.bookmarks,
            collections: local.collections,
        };
    };

    let mut books: HashMap<String, CloudBook> = HashMap::new();
    for book in local.books {
        books.insert(book.id.clone(), book);
    }
    for incoming in remote.books {
        match books.remove(&incoming.id) {
            Some(existing) => {
                books.insert(incoming.id.clone(), merge_book(existing, incoming));
            }
            None => {
                books.insert(incoming.id.clone(), incoming);
            }
        }
    }

    let mut bookmarks: HashMap<String, Bookmark> = HashMap::new();
    for bookmark in local.bookmarks {
        bookmarks.insert(bookmark.id.clone(), bookmark);
    }
    for incoming in remote.bookmarks {
        match bookmarks.remove(&incoming.id) {
            Some(existing) => {
                let winner = if incoming.updated_at > existing.updated_at {
                    incoming
                } else {
                    existing
                };
                bookmarks.insert(winner.id.clone(), winner);
            }
            None => {
                bookmarks.insert(incoming.id.clone(), incoming);
            }
        }
    }

    let mut collections: HashMap<String, Collection> = HashMap::new();
    for collection in local.collections {
        collections.insert(collection.id.clone(), collection);
    }
    for incoming in remote.collections {
        match collections.remove(&incoming.id) {
            Some(existing) => {
                let winner = if incoming.updated_at >= existing.updated_at {
                    incoming
                } else {
                    existing
                };
                collections.insert(winner.id.clone(), winner);
            }
            None => {
                collections.insert(incoming.id.clone(), incoming);
            }
        }
    }

    let mut merged_books: Vec<CloudBook> = books.into_values().collect();
    merged_books.sort_by(|left, right| {
        right
            .last_opened_at
            .cmp(&left.last_opened_at)
            .then_with(|| left.title.cmp(&right.title))
    });
    let mut merged_bookmarks: Vec<Bookmark> = bookmarks.into_values().collect();
    merged_bookmarks
        .sort_by_key(|bookmark| (bookmark.chapter, (bookmark.fraction * 1000.0) as u32));
    let mut merged_collections: Vec<Collection> = collections.into_values().collect();
    merged_collections.sort_by(|left, right| left.name.cmp(&right.name));

    CloudSnapshot {
        schema: CLOUD_SCHEMA,
        device_id: local.device_id,
        updated_at: now_secs(),
        books: merged_books,
        bookmarks: merged_bookmarks,
        collections: merged_collections,
    }
}

fn merge_book(existing: CloudBook, incoming: CloudBook) -> CloudBook {
    let incoming_is_newer = incoming.progress.updated_at > existing.progress.updated_at
        || (incoming.progress.updated_at == existing.progress.updated_at
            && incoming.last_opened_at > existing.last_opened_at);

    let mut winner = if incoming_is_newer {
        incoming.clone()
    } else {
        existing.clone()
    };
    let other = if incoming_is_newer {
        existing
    } else {
        incoming
    };

    if winner.title.trim().is_empty() {
        winner.title = other.title;
    }
    if winner.author.trim().is_empty() {
        winner.author = other.author;
    }
    winner.chapter_count = winner.chapter_count.max(other.chapter_count);
    winner.added_at = winner.added_at.min(other.added_at);
    if winner.last_opened_at.is_none() {
        winner.last_opened_at = other.last_opened_at;
    }
    if winner.collection_id.is_none() {
        winner.collection_id = other.collection_id;
    }
    winner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud_book(id: &str, chapter: usize, updated_at: u64) -> CloudBook {
        CloudBook {
            id: id.to_owned(),
            title: "测试书".to_owned(),
            author: "作者".to_owned(),
            collection_id: None,
            format: BookFormat::Epub,
            chapter_count: 10,
            progress: ReadingProgress {
                chapter,
                fraction: 0.5,
                scroll_offset: 0.0,
                updated_at,
            },
            added_at: 1,
            last_opened_at: Some(updated_at),
        }
    }

    #[test]
    fn merge_prefers_newer_progress_and_bookmark() {
        let local = CloudSnapshot {
            schema: CLOUD_SCHEMA,
            device_id: "local".to_owned(),
            updated_at: 1,
            books: vec![cloud_book("a", 1, 10)],
            bookmarks: vec![Bookmark {
                id: "b1".to_owned(),
                book_id: "a".to_owned(),
                chapter: 1,
                fraction: 0.2,
                label: "旧书签".to_owned(),
                created_at: 1,
                updated_at: 10,
                deleted: false,
            }],
            collections: Vec::new(),
        };
        let remote = CloudSnapshot {
            schema: CLOUD_SCHEMA,
            device_id: "remote".to_owned(),
            updated_at: 2,
            books: vec![cloud_book("a", 8, 20), cloud_book("b", 3, 30)],
            bookmarks: vec![Bookmark {
                id: "b1".to_owned(),
                book_id: "a".to_owned(),
                chapter: 8,
                fraction: 0.8,
                label: "新书签".to_owned(),
                created_at: 1,
                updated_at: 20,
                deleted: false,
            }],
            collections: Vec::new(),
        };

        let merged = merge_snapshots(local, Some(remote));
        assert_eq!(merged.books.len(), 2);
        let a = merged.books.iter().find(|book| book.id == "a");
        assert_eq!(a.map(|book| book.progress.chapter), Some(8));
        assert_eq!(merged.bookmarks.len(), 1);
        assert_eq!(
            merged.bookmarks.first().map(|bookmark| bookmark.chapter),
            Some(8)
        );
    }

    #[test]
    fn applying_cloud_snapshot_adds_cloud_only_placeholder() {
        let mut library = LibraryData::default();
        let snapshot = CloudSnapshot {
            schema: CLOUD_SCHEMA,
            device_id: "remote".to_owned(),
            updated_at: 1,
            books: vec![cloud_book("cloud", 2, 30)],
            bookmarks: Vec::new(),
            collections: Vec::new(),
        };
        snapshot.apply_to_library(&mut library);
        assert_eq!(library.books.len(), 1);
        assert!(
            library
                .books
                .first()
                .is_some_and(|book| book.path.is_none())
        );
        assert_eq!(
            library.books.first().map(|book| book.progress.chapter),
            Some(2)
        );
    }
}
