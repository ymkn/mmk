use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, Transaction};

use crate::models::{
    AmazonContinueSeriesItemInput, AmazonContinueSeriesItemView, BookMetadataInput, BookView,
    ImportedBook, SeriesMetadataInput, SyncResult,
};
use crate::series;

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .iter()
        .any(|name| name == column))
}

impl Database {
    pub fn initialize(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let database = Self { path };
        let connection = database.connect()?;
        connection
            .execute_batch(SCHEMA)
            .map_err(|error| error.to_string())?;
        if !table_has_column(&connection, "books", "label")? {
            connection
                .execute("ALTER TABLE books ADD COLUMN label TEXT", [])
                .map_err(|error| error.to_string())?;
        }
        if !table_has_column(&connection, "amazon_continue_series_items", "image_url")? {
            connection
                .execute(
                    "ALTER TABLE amazon_continue_series_items ADD COLUMN image_url TEXT",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
        if !table_has_column(&connection, "amazon_continue_series_items", "has_rating")? {
            connection
                .execute(
                    "ALTER TABLE amazon_continue_series_items ADD COLUMN has_rating INTEGER NOT NULL DEFAULT 0",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(database)
    }

    fn connect(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path).map_err(|error| error.to_string())?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .map_err(|error| error.to_string())?;
        Ok(connection)
    }

    pub fn sync(
        &self,
        source_path: &Path,
        books: &[ImportedBook],
        excluded: usize,
    ) -> Result<SyncResult, String> {
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let existing: HashSet<String> = {
            let mut statement = transaction
                .prepare("SELECT asin FROM books")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<_, _>>()
                .map_err(|error| error.to_string())?
        };
        transaction
            .execute("UPDATE books SET source_status = 'missing'", [])
            .map_err(|error| error.to_string())?;
        let now = Utc::now().to_rfc3339();
        for book in books {
            transaction
                .execute(
                    UPSERT_BOOK,
                    params![
                        book.asin,
                        book.title,
                        serde_json::to_string(&book.authors).map_err(|error| error.to_string())?,
                        book.publisher,
                        book.publication_date,
                        book.acquisition_date,
                        book.content_type,
                        book.ownership_type,
                        book.source_origin,
                        book.cover_path,
                        book.inferred_series_name,
                        book.inferred_series_index,
                        book.label,
                        now,
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
        let missing_books: usize = transaction
            .query_row(
                "SELECT COUNT(*) FROM books WHERE source_status = 'missing'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT INTO sync_state (id, source_path, synced_at) VALUES (1, ?1, ?2) ON CONFLICT(id) DO UPDATE SET source_path=excluded.source_path, synced_at=excluded.synced_at",
            params![source_path.to_string_lossy(), now],
        ).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        let inserted_books = books
            .iter()
            .filter(|book| !existing.contains(&book.asin))
            .count();
        Ok(SyncResult {
            source_path: source_path.to_string_lossy().into_owned(),
            active_books: books.len(),
            inserted_books,
            updated_books: books.len() - inserted_books,
            excluded_books: excluded,
            missing_books,
        })
    }

    pub fn list_books(&self) -> Result<Vec<BookView>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(LIST_BOOKS)
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                let authors_json: String = row.get(3)?;
                let manual_authors: Option<String> = row.get(20)?;
                let authors = manual_authors
                    .as_ref()
                    .map(|value| split_authors(value))
                    .unwrap_or_else(|| serde_json::from_str(&authors_json).unwrap_or_default());
                let tags: Option<String> = row.get(22)?;
                Ok(BookView {
                    id: row.get(0)?,
                    asin: row.get(1)?,
                    title: row.get(2)?,
                    authors,
                    publisher: row.get(4)?,
                    publication_date: row.get(5)?,
                    acquisition_date: row.get(6)?,
                    ownership_type: row.get(7)?,
                    source_origin: row.get(8)?,
                    source_status: row.get(9)?,
                    cover_path: row.get(10)?,
                    score: row.get(11)?,
                    favorite: row.get::<_, i64>(12)? != 0,
                    series_mode: row.get(13)?,
                    inferred_series_name: row.get(14)?,
                    inferred_series_index: row.get(15)?,
                    manual_series_name: row.get(16)?,
                    manual_series_index: row.get(17)?,
                    effective_series_name: row.get(18)?,
                    effective_series_index: row.get(19)?,
                    manual_title: row.get(21)?,
                    manual_authors,
                    manual_publisher: row.get(23)?,
                    label: row.get(24)?,
                    tags: tags
                        .map(|value| value.split('\u{1f}').map(str::to_owned).collect())
                        .unwrap_or_default(),
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<_, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn save_metadata(&self, input: BookMetadataInput) -> Result<(), String> {
        if !matches!(input.series_mode.as_str(), "auto" | "manual" | "none") {
            return Err("seriesMode が不正です".to_owned());
        }
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let now = Utc::now().to_rfc3339();
        transaction
            .execute(
                SAVE_METADATA,
                params![
                    input.book_id,
                    input.score,
                    input.favorite,
                    input.series_mode,
                    trim_option(input.manual_series_name),
                    input.manual_series_index,
                    trim_option(input.manual_title),
                    trim_option(input.manual_authors),
                    trim_option(input.manual_publisher),
                    now
                ],
            )
            .map_err(|error| error.to_string())?;
        replace_tags(&transaction, input.book_id, input.tags)?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn adjust_score(&self, book_id: i64, delta: i64) -> Result<i64, String> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO user_book_metadata (book_id, score, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(book_id) DO UPDATE SET score = score + excluded.score, updated_at = excluded.updated_at",
            params![book_id, delta, now],
        ).map_err(|error| error.to_string())?;
        connection
            .query_row(
                "SELECT score FROM user_book_metadata WHERE book_id=?1",
                [book_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }

    pub fn set_favorite(&self, book_id: i64, favorite: bool) -> Result<(), String> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO user_book_metadata (book_id, favorite, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(book_id) DO UPDATE SET favorite=excluded.favorite, updated_at=excluded.updated_at",
            params![book_id, favorite, now],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn update_series_metadata(&self, input: SeriesMetadataInput) -> Result<(), String> {
        let mut book_ids = input.book_ids;
        book_ids.sort_unstable();
        book_ids.dedup();
        if book_ids.is_empty() {
            return Err("更新対象の書籍がありません".to_owned());
        }
        if !(-1..=1).contains(&input.score_delta) {
            return Err("評価の変更量は -1、0、1 のいずれかで指定してください".to_owned());
        }
        let rename_to = match input.rename_to {
            Some(value) => Some(
                trim_option(Some(value))
                    .ok_or_else(|| "シリーズ名を空にはできません".to_owned())?,
            ),
            None => None,
        };

        let mut connection = self.connect()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let now = Utc::now().to_rfc3339();
        for book_id in &book_ids {
            let exists = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM books WHERE id=?1)",
                    [book_id],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| error.to_string())?;
            if !exists {
                return Err(format!("書籍 ID {book_id} が見つかりません"));
            }

            if input.score_delta != 0 {
                transaction.execute(
                    "INSERT INTO user_book_metadata (book_id, score, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(book_id) DO UPDATE SET score=score+excluded.score, updated_at=excluded.updated_at",
                    params![book_id, input.score_delta, now],
                ).map_err(|error| error.to_string())?;
            }
            if let Some(favorite) = input.favorite {
                transaction.execute(
                    "INSERT INTO user_book_metadata (book_id, favorite, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(book_id) DO UPDATE SET favorite=excluded.favorite, updated_at=excluded.updated_at",
                    params![book_id, favorite, now],
                ).map_err(|error| error.to_string())?;
            }
            if let Some(series_name) = &rename_to {
                transaction.execute(
                    "INSERT INTO user_book_metadata (book_id, series_mode, manual_series_name, manual_series_index, updated_at) SELECT id, 'manual', ?2, inferred_series_index, ?3 FROM books WHERE id=?1 ON CONFLICT(book_id) DO UPDATE SET series_mode='manual', manual_series_name=excluded.manual_series_name, manual_series_index=COALESCE(user_book_metadata.manual_series_index, excluded.manual_series_index), updated_at=excluded.updated_at",
                    params![book_id, series_name, now],
                ).map_err(|error| error.to_string())?;
            }
            add_tags(&transaction, *book_id, &input.tags_to_add)?;
            remove_tags(&transaction, *book_id, &input.tags_to_remove)?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn replace_amazon_continue_series(
        &self,
        items: Vec<AmazonContinueSeriesItemInput>,
    ) -> Result<usize, String> {
        if items.len() > 500 {
            return Err("Amazonの候補数が多すぎます".to_owned());
        }

        let mut connection = self.connect()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let now = Utc::now().to_rfc3339();
        transaction
            .execute("DELETE FROM amazon_continue_series_items", [])
            .map_err(|error| error.to_string())?;

        let mut stored = 0;
        let mut seen_asins = HashSet::new();
        for item in items {
            let asin = item.asin.trim().to_ascii_uppercase();
            if !is_valid_asin(&asin) || !seen_asins.insert(asin.clone()) {
                continue;
            }
            let title = item.title.trim();
            if title.is_empty() || title.chars().count() > 1000 {
                continue;
            }
            let Some(price) = item.price.filter(|value| *value >= 0) else {
                continue;
            };
            let image_url = normalize_amazon_image_url(item.image_url).or_else(|| {
                Some(format!(
                    "https://images-na.ssl-images-amazon.com/images/P/{asin}.01.LZZZZZZZ.jpg"
                ))
            });
            let inferred = series::infer_candidate(title);
            let series_name = inferred
                .as_ref()
                .map(|candidate| candidate.display_name.as_str());
            let series_index = inferred.as_ref().map(|candidate| candidate.volume_index);
            let product_url = format!("https://www.amazon.co.jp/dp/{asin}");

            transaction
                .execute(
                    "INSERT INTO amazon_continue_series_items (asin, title, product_url, price, has_rating, image_url, series_name, series_index, seen_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    rusqlite::params![asin, title, product_url, price, item.has_rating, image_url, series_name, series_index, now],
                )
                .map_err(|error| error.to_string())?;
            stored += 1;
        }

        transaction.commit().map_err(|error| error.to_string())?;
        Ok(stored)
    }

    pub fn list_amazon_continue_series(&self) -> Result<Vec<AmazonContinueSeriesItemView>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT asin, title, product_url, price, has_rating, image_url, series_name, series_index, seen_at FROM amazon_continue_series_items WHERE price IS NOT NULL ORDER BY COALESCE(series_name, title) COLLATE NOCASE, series_index, title COLLATE NOCASE, asin",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(AmazonContinueSeriesItemView {
                    asin: row.get(0)?,
                    title: row.get(1)?,
                    product_url: row.get(2)?,
                    price: row.get(3)?,
                    has_rating: row.get::<_, i64>(4)? != 0,
                    image_url: row.get(5)?,
                    series_name: row.get(6)?,
                    volume_index: row.get(7)?,
                    seen_at: row.get(8)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<_, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn clear_amazon_continue_series(&self) -> Result<(), String> {
        let connection = self.connect()?;
        connection
            .execute("DELETE FROM amazon_continue_series_items", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn add_tags(transaction: &Transaction<'_>, book_id: i64, tags: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for tag in tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        if !seen.insert(tag.to_lowercase()) {
            continue;
        }
        transaction
            .execute(
                "INSERT INTO tags(name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
                [tag],
            )
            .map_err(|error| error.to_string())?;
        let tag_id: i64 = transaction
            .query_row(
                "SELECT id FROM tags WHERE name=?1 COLLATE NOCASE",
                [tag],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO book_tags(book_id, tag_id) VALUES (?1, ?2)",
                params![book_id, tag_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_tags(transaction: &Transaction<'_>, book_id: i64, tags: &[String]) -> Result<(), String> {
    for tag in tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        transaction
            .execute(
                "DELETE FROM book_tags WHERE book_id=?1 AND tag_id IN (SELECT id FROM tags WHERE name=?2 COLLATE NOCASE)",
                params![book_id, tag],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn replace_tags(
    transaction: &Transaction<'_>,
    book_id: i64,
    tags: Vec<String>,
) -> Result<(), String> {
    transaction
        .execute("DELETE FROM book_tags WHERE book_id=?1", [book_id])
        .map_err(|error| error.to_string())?;
    let mut seen = HashSet::new();
    for tag in tags
        .into_iter()
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty())
    {
        if !seen.insert(tag.to_lowercase()) {
            continue;
        }
        transaction
            .execute(
                "INSERT INTO tags(name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
                [&tag],
            )
            .map_err(|error| error.to_string())?;
        let tag_id: i64 = transaction
            .query_row(
                "SELECT id FROM tags WHERE name=?1 COLLATE NOCASE",
                [&tag],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO book_tags(book_id, tag_id) VALUES (?1, ?2)",
                params![book_id, tag_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn trim_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn split_authors(value: &str) -> Vec<String> {
    value
        .split([',', '、', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_valid_asin(asin: &str) -> bool {
    asin.len() == 10
        && asin
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS books (
  id INTEGER PRIMARY KEY,
  asin TEXT NOT NULL UNIQUE,
  source_title TEXT NOT NULL,
  source_authors_json TEXT NOT NULL DEFAULT '[]',
  source_publisher TEXT,
  publication_date TEXT,
  acquisition_date TEXT,
  content_type TEXT,
  ownership_type TEXT NOT NULL DEFAULT 'unknown',
  source_origin TEXT NOT NULL DEFAULT '',
  cover_path TEXT,
  inferred_series_name TEXT,
  inferred_series_index REAL,
  label TEXT,
  first_seen_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  source_status TEXT NOT NULL DEFAULT 'active'
);
CREATE TABLE IF NOT EXISTS user_book_metadata (
  book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
  score INTEGER NOT NULL DEFAULT 0,
  favorite INTEGER NOT NULL DEFAULT 0,
  series_mode TEXT NOT NULL DEFAULT 'auto' CHECK(series_mode IN ('auto','manual','none')),
  manual_series_name TEXT,
  manual_series_index REAL,
  manual_title TEXT,
  manual_authors TEXT,
  manual_publisher TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE);
CREATE TABLE IF NOT EXISTS book_tags (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY(book_id, tag_id)
);
CREATE TABLE IF NOT EXISTS sync_state (id INTEGER PRIMARY KEY CHECK(id=1), source_path TEXT NOT NULL, synced_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS books_source_status ON books(source_status);
CREATE INDEX IF NOT EXISTS books_inferred_series ON books(inferred_series_name, inferred_series_index);
CREATE TABLE IF NOT EXISTS amazon_continue_series_items (
  asin TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  product_url TEXT NOT NULL,
  price INTEGER,
  has_rating INTEGER NOT NULL DEFAULT 0,
  image_url TEXT,
  series_name TEXT,
  series_index REAL,
  seen_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS amazon_continue_series_series ON amazon_continue_series_items(series_name, series_index);
"#;

fn normalize_amazon_image_url(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_owned();
    if value.len() > 2048 {
        return None;
    }
    let url = tauri::Url::parse(&value).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    match url.host_str()?.to_ascii_lowercase().as_str() {
        "m.media-amazon.com" | "images-na.ssl-images-amazon.com" => Some(url.to_string()),
        _ => None,
    }
}

const UPSERT_BOOK: &str = r#"
INSERT INTO books (asin, source_title, source_authors_json, source_publisher, publication_date,
 acquisition_date, content_type, ownership_type, source_origin, cover_path,
 inferred_series_name, inferred_series_index, label, first_seen_at, last_seen_at, source_status)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14,'active')
ON CONFLICT(asin) DO UPDATE SET
 source_title=excluded.source_title, source_authors_json=excluded.source_authors_json,
 source_publisher=excluded.source_publisher, publication_date=excluded.publication_date,
 acquisition_date=excluded.acquisition_date, content_type=excluded.content_type,
 ownership_type=excluded.ownership_type, source_origin=excluded.source_origin,
 cover_path=excluded.cover_path, inferred_series_name=excluded.inferred_series_name,
 inferred_series_index=excluded.inferred_series_index, label=excluded.label,
 last_seen_at=excluded.last_seen_at, source_status='active'
"#;

const LIST_BOOKS: &str = r#"
SELECT b.id, b.asin, COALESCE(NULLIF(u.manual_title,''), b.source_title), b.source_authors_json,
 COALESCE(NULLIF(u.manual_publisher,''), b.source_publisher), b.publication_date, b.acquisition_date,
 b.ownership_type, b.source_origin, b.source_status, b.cover_path,
 COALESCE(u.score,0), COALESCE(u.favorite,0), COALESCE(u.series_mode,'auto'),
 b.inferred_series_name, b.inferred_series_index, u.manual_series_name, u.manual_series_index,
 CASE COALESCE(u.series_mode,'auto') WHEN 'manual' THEN u.manual_series_name WHEN 'none' THEN NULL ELSE b.inferred_series_name END,
 CASE COALESCE(u.series_mode,'auto') WHEN 'manual' THEN u.manual_series_index WHEN 'none' THEN NULL ELSE b.inferred_series_index END,
 u.manual_authors, u.manual_title,
 (SELECT group_concat(t.name, char(31)) FROM book_tags bt JOIN tags t ON t.id=bt.tag_id WHERE bt.book_id=b.id),
 u.manual_publisher, b.label
FROM books b LEFT JOIN user_book_metadata u ON u.book_id=b.id
ORDER BY COALESCE(NULLIF(u.manual_title,''), b.source_title) COLLATE NOCASE, b.asin
"#;

const SAVE_METADATA: &str = r#"
INSERT INTO user_book_metadata (book_id, score, favorite, series_mode, manual_series_name,
 manual_series_index, manual_title, manual_authors, manual_publisher, updated_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
ON CONFLICT(book_id) DO UPDATE SET score=excluded.score, favorite=excluded.favorite,
 series_mode=excluded.series_mode, manual_series_name=excluded.manual_series_name,
 manual_series_index=excluded.manual_series_index, manual_title=excluded.manual_title,
 manual_authors=excluded.manual_authors, manual_publisher=excluded.manual_publisher,
 updated_at=excluded.updated_at
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_database() -> (Database, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mmk-{unique}.sqlite3"));
        (Database::initialize(path.clone()).unwrap(), path)
    }

    fn imported(title: &str) -> ImportedBook {
        imported_with("B012345678", title, 1.0)
    }

    fn imported_with(asin: &str, title: &str, series_index: f64) -> ImportedBook {
        ImportedBook {
            asin: asin.to_owned(),
            title: title.to_owned(),
            authors: vec!["著者".to_owned()],
            publisher: Some("出版社".to_owned()),
            publication_date: None,
            acquisition_date: None,
            content_type: Some("application/x-mobipocket-ebook".to_owned()),
            ownership_type: "purchased".to_owned(),
            source_origin: "Purchase".to_owned(),
            cover_path: None,
            inferred_series_name: Some("星巡り".to_owned()),
            inferred_series_index: Some(series_index),
            label: Some("星巡りレーベル".to_owned()),
        }
    }

    #[test]
    fn source_sync_preserves_user_metadata() {
        let (database, path) = test_database();
        database
            .sync(Path::new("metadata.xml"), &[imported("星巡り 1")], 0)
            .unwrap();
        let book_id = database.list_books().unwrap()[0].id;
        database
            .save_metadata(BookMetadataInput {
                book_id,
                score: 3,
                favorite: true,
                series_mode: "manual".to_owned(),
                manual_series_name: Some("手動シリーズ".to_owned()),
                manual_series_index: Some(7.0),
                manual_title: Some("手動タイトル".to_owned()),
                manual_authors: None,
                manual_publisher: None,
                tags: vec!["再読".to_owned(), "資料".to_owned()],
            })
            .unwrap();

        database
            .sync(
                Path::new("metadata.xml"),
                &[imported("変更された同期タイトル")],
                0,
            )
            .unwrap();
        let book = database.list_books().unwrap().remove(0);
        assert_eq!(book.title, "手動タイトル");
        assert_eq!(book.score, 3);
        assert!(book.favorite);
        assert_eq!(book.effective_series_name.as_deref(), Some("手動シリーズ"));
        assert_eq!(book.tags, vec!["再読", "資料"]);
        drop(database);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_source_is_marked_without_deletion() {
        let (database, path) = test_database();
        database
            .sync(Path::new("metadata.xml"), &[imported("星巡り 1")], 0)
            .unwrap();
        let result = database.sync(Path::new("metadata.xml"), &[], 0).unwrap();
        assert_eq!(result.missing_books, 1);
        assert_eq!(database.list_books().unwrap()[0].source_status, "missing");
        drop(database);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn sync_persists_and_updates_label() {
        let (database, path) = test_database();
        let mut book = imported("星巡り 1");
        database
            .sync(Path::new("metadata.xml"), std::slice::from_ref(&book), 0)
            .unwrap();
        assert_eq!(
            database.list_books().unwrap()[0].label.as_deref(),
            Some("星巡りレーベル")
        );

        book.label = None;
        database
            .sync(Path::new("metadata.xml"), std::slice::from_ref(&book), 0)
            .unwrap();
        assert_eq!(database.list_books().unwrap()[0].label, None);
        drop(database);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn series_update_applies_to_each_book_atomically() {
        let (database, path) = test_database();
        database
            .sync(
                Path::new("metadata.xml"),
                &[
                    imported_with("B012345671", "星巡り 1", 1.0),
                    imported_with("B012345672", "星巡り 2", 2.0),
                ],
                0,
            )
            .unwrap();
        let book_ids: Vec<_> = database
            .list_books()
            .unwrap()
            .into_iter()
            .map(|book| book.id)
            .collect();

        database
            .save_metadata(BookMetadataInput {
                book_id: book_ids[0],
                score: 0,
                favorite: false,
                series_mode: "manual".to_owned(),
                manual_series_name: Some("旧シリーズ名".to_owned()),
                manual_series_index: Some(9.0),
                manual_title: None,
                manual_authors: None,
                manual_publisher: None,
                tags: Vec::new(),
            })
            .unwrap();

        database
            .update_series_metadata(SeriesMetadataInput {
                book_ids,
                score_delta: 1,
                favorite: Some(true),
                rename_to: Some("星巡り・新装版".to_owned()),
                tags_to_add: vec!["全巻所有".to_owned()],
                tags_to_remove: Vec::new(),
            })
            .unwrap();

        let books = database.list_books().unwrap();
        assert_eq!(books.len(), 2);
        assert!(books.iter().all(|book| book.score == 1 && book.favorite));
        assert!(books
            .iter()
            .all(|book| book.effective_series_name.as_deref() == Some("星巡り・新装版")));
        assert_eq!(books[0].effective_series_index, Some(9.0));
        assert_eq!(books[1].effective_series_index, Some(2.0));
        assert!(books.iter().all(|book| book.tags == vec!["全巻所有"]));
        drop(database);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn series_update_rolls_back_when_any_book_is_missing() {
        let (database, path) = test_database();
        database
            .sync(
                Path::new("metadata.xml"),
                &[
                    imported_with("B012345671", "星巡り 1", 1.0),
                    imported_with("B012345672", "星巡り 2", 2.0),
                ],
                0,
            )
            .unwrap();
        let book_ids: Vec<_> = database
            .list_books()
            .unwrap()
            .into_iter()
            .map(|book| book.id)
            .collect();

        let error = database.update_series_metadata(SeriesMetadataInput {
            book_ids: vec![book_ids[0], book_ids[0], book_ids[1], 999_999],
            score_delta: 1,
            favorite: Some(true),
            rename_to: Some("更新後シリーズ".to_owned()),
            tags_to_add: vec!["ロールバック確認".to_owned()],
            tags_to_remove: Vec::new(),
        });

        assert!(error.is_err());
        let books = database.list_books().unwrap();
        assert!(books.iter().all(|book| {
            book.score == 0
                && !book.favorite
                && book.effective_series_name.as_deref() == Some("星巡り")
                && book.tags.is_empty()
        }));
        drop(database);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn amazon_recommendations_are_replaced_and_normalized() {
        let (database, path) = test_database();
        let stored = database
            .replace_amazon_continue_series(vec![
                AmazonContinueSeriesItemInput {
                    asin: "b012345679".to_owned(),
                    title: "星巡り 第3巻".to_owned(),
                    price: Some(480),
                    has_rating: true,
                    image_url: None,
                },
                AmazonContinueSeriesItemInput {
                    asin: "b012345679".to_owned(),
                    title: "重複した候補 3巻".to_owned(),
                    price: Some(500),
                    has_rating: false,
                    image_url: Some("https://example.com/untrusted.jpg".to_owned()),
                },
                AmazonContinueSeriesItemInput {
                    asin: "not-an-asin".to_owned(),
                    title: "不正な候補".to_owned(),
                    price: None,
                    has_rating: false,
                    image_url: None,
                },
                AmazonContinueSeriesItemInput {
                    asin: "b012345680".to_owned(),
                    title: "星巡り 第4巻".to_owned(),
                    price: None,
                    has_rating: false,
                    image_url: None,
                },
            ])
            .unwrap();

        assert_eq!(stored, 1);
        let items = database.list_amazon_continue_series().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].asin, "B012345679");
        assert_eq!(
            items[0].product_url,
            "https://www.amazon.co.jp/dp/B012345679"
        );
        assert_eq!(items[0].series_name.as_deref(), Some("星巡り"));
        assert_eq!(items[0].volume_index, Some(3.0));
        assert!(items[0].has_rating);
        assert_eq!(
            items[0].image_url.as_deref(),
            Some("https://images-na.ssl-images-amazon.com/images/P/B012345679.01.LZZZZZZZ.jpg")
        );

        database.clear_amazon_continue_series().unwrap();
        assert!(database.list_amazon_continue_series().unwrap().is_empty());
        drop(database);
        let _ = fs::remove_file(path);
    }
}
