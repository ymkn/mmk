use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use quick_xml::de::from_reader;
use serde::Deserialize;

use crate::labels;
use crate::models::ImportedBook;
use crate::series;

const METADATA_FILE_NAME: &str = "KindleSyncMetadataCache.xml";

#[derive(Debug, Deserialize)]
struct Response {
    add_update_list: AddUpdateList,
}

#[derive(Debug, Deserialize)]
struct AddUpdateList {
    #[serde(rename = "meta_data", default)]
    books: Vec<RawBook>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawBook {
    #[serde(rename = "ASIN")]
    asin: String,
    title: String,
    #[serde(default)]
    authors: Authors,
    #[serde(default)]
    publishers: Publishers,
    #[serde(default)]
    origins: Origins,
    publication_date: Option<String>,
    purchase_date: Option<String>,
    content_type: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct Authors {
    #[serde(rename = "author", default)]
    values: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct Publishers {
    #[serde(rename = "publisher", default)]
    values: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct Origins {
    #[serde(rename = "origin", default)]
    values: Vec<Origin>,
}

#[derive(Debug, Deserialize, Clone)]
struct Origin {
    #[serde(rename = "type", default)]
    kind: String,
}

pub fn locate_metadata(configured_profile_path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(configured_profile_path) = configured_profile_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let configured_path = PathBuf::from(configured_profile_path);
        let metadata_path = if configured_path.is_dir()
            || configured_path.file_name().and_then(|name| name.to_str())
                != Some(METADATA_FILE_NAME)
        {
            configured_path.join(METADATA_FILE_NAME)
        } else {
            configured_path
        };
        if metadata_path.is_file() {
            return Ok(metadata_path);
        }
        return Err(format!(
            "{} に {} が見つかりません",
            metadata_path.parent().unwrap_or(Path::new(".")).display(),
            METADATA_FILE_NAME
        ));
    }

    let local_app_data = env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA が設定されていません")?;
    let packages = PathBuf::from(local_app_data).join("Packages");
    let mut candidates = Vec::new();
    let entries =
        fs::read_dir(&packages).map_err(|error| format!("{}: {error}", packages.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.contains("kindle") {
            continue;
        }
        let candidate = entry.path().join(format!(
            "LocalState/Classic/Data/Cache/{METADATA_FILE_NAME}"
        ));
        if candidate.is_file() {
            candidates.push(candidate);
        }
    }
    candidates.sort_by_key(|path| fs::metadata(path).and_then(|value| value.modified()).ok());
    candidates
        .pop()
        .ok_or_else(|| format!("{METADATA_FILE_NAME} が見つかりません"))
}

pub fn load_books(metadata_path: &Path) -> Result<(Vec<ImportedBook>, usize), String> {
    let file = File::open(metadata_path)
        .map_err(|error| format!("{}: {error}", metadata_path.display()))?;
    let response: Response = from_reader(BufReader::new(file))
        .map_err(|error| format!("Kindle metadata XML の解析に失敗しました: {error}"))?;

    let mut grouped: HashMap<String, Vec<RawBook>> = HashMap::new();
    for book in response.add_update_list.books {
        if !book.asin.trim().is_empty() {
            grouped.entry(book.asin.clone()).or_default().push(book);
        }
    }

    let mut excluded = 0;
    let mut selected = Vec::new();
    for mut rows in grouped.into_values() {
        rows.sort_by_key(|row| std::cmp::Reverse(preference(row)));
        if rows.iter().all(is_excluded) {
            excluded += 1;
            continue;
        }
        if let Some(row) = rows.into_iter().find(|row| !is_excluded(row)) {
            selected.push(row);
        }
    }
    selected.sort_by(|left, right| left.asin.cmp(&right.asin));

    let titles: Vec<_> = selected.iter().map(|row| row.title.clone()).collect();
    let inferred = series::infer_library(&titles);
    let covers = metadata_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("covers");
    let books = selected
        .into_iter()
        .zip(inferred)
        .map(|(row, inferred)| {
            let origins = origin_types(&row);
            let ownership_type = normalize_ownership(&origins).to_owned();
            let cover_name = format!("{:X}.jpg", md5::compute(row.asin.as_bytes()));
            let cover_path = covers.join(cover_name);
            let label = labels::extract_label(&row.title).map(str::to_owned);
            ImportedBook {
                asin: row.asin,
                title: row.title,
                authors: clean_values(row.authors.values),
                publisher: clean_values(row.publishers.values).into_iter().next(),
                publication_date: clean_optional(row.publication_date),
                acquisition_date: clean_optional(row.purchase_date),
                content_type: clean_optional(row.content_type),
                ownership_type,
                source_origin: origins.into_iter().collect::<Vec<_>>().join(","),
                cover_path: cover_path
                    .is_file()
                    .then(|| cover_path.to_string_lossy().into_owned()),
                inferred_series_name: inferred.as_ref().map(|value| value.name.clone()),
                inferred_series_index: inferred.map(|value| value.volume_index),
                label,
            }
        })
        .collect();
    Ok((books, excluded))
}

fn clean_values(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && seen.insert(value.to_lowercase()))
        .collect()
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn origin_types(row: &RawBook) -> HashSet<String> {
    row.origins
        .values
        .iter()
        .map(|origin| origin.kind.trim().to_owned())
        .filter(|origin| !origin.is_empty())
        .collect()
}

fn is_excluded(row: &RawBook) -> bool {
    let origins = origin_types(row);
    !origins.is_empty()
        && origins
            .iter()
            .all(|origin| matches!(origin.as_str(), "Sample" | "KindleDictionary"))
}

fn preference(row: &RawBook) -> u8 {
    let origins = origin_types(row);
    if origins.contains("Purchase") {
        4
    } else if origins.contains("KindleUnlimited") {
        3
    } else if origins.contains("Prime") {
        2
    } else if is_excluded(row) {
        0
    } else {
        1
    }
}

fn normalize_ownership(origins: &HashSet<String>) -> &'static str {
    if origins.contains("Purchase") {
        "purchased"
    } else if origins.contains("KindleUnlimited") || origins.contains("Prime") {
        "subscription"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parser_deduplicates_purchase_and_excludes_non_library_items() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cache = std::env::temp_dir()
            .join(format!("mmk-kindle-{unique}"))
            .join("Cache");
        fs::create_dir_all(cache.join("covers")).unwrap();
        let xml_path = cache.join("KindleSyncMetadataCache.xml");
        let xml = r#"<response><add_update_list>
          <meta_data><ASIN>B012345678</ASIN><title>星巡り 第1巻</title><authors><author>著者</author></authors><publishers><publisher>出版社</publisher></publishers><origins><origin><type>Sample</type></origin></origins><publication_date/><purchase_date/><content_type>ebook</content_type></meta_data>
          <meta_data><ASIN>B012345678</ASIN><title>星巡り 第1巻</title><authors><author>著者</author></authors><publishers><publisher>出版社</publisher></publishers><origins><origin><type>Purchase</type></origin></origins><publication_date>2026-01-01</publication_date><purchase_date>2026-02-01</purchase_date><content_type>ebook</content_type></meta_data>
          <meta_data><ASIN>B012345679</ASIN><title>Sample</title><authors/><publishers/><origins><origin><type>Sample</type></origin></origins><publication_date/><purchase_date/><content_type>ebook</content_type></meta_data>
          <meta_data><ASIN>B012345670</ASIN><title>Dictionary</title><authors/><publishers/><origins><origin><type>KindleDictionary</type></origin></origins><publication_date/><purchase_date/><content_type>ebook</content_type></meta_data>
        </add_update_list></response>"#;
        fs::write(&xml_path, xml).unwrap();
        let cover_name = format!("{:X}.jpg", md5::compute(b"B012345678"));
        fs::write(cache.join("covers").join(cover_name), b"image").unwrap();

        let (books, excluded) = load_books(&xml_path).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(excluded, 2);
        assert_eq!(books[0].ownership_type, "purchased");
        assert_eq!(books[0].source_origin, "Purchase");
        assert_eq!(books[0].inferred_series_name.as_deref(), Some("星巡り"));
        assert!(books[0].cover_path.is_some());
        let root = cache.parent().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_profile_directory_resolves_metadata_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let profile = std::env::temp_dir().join(format!("mmk-profile-{unique}"));
        fs::create_dir_all(&profile).unwrap();
        let metadata = profile.join(METADATA_FILE_NAME);
        fs::write(&metadata, b"metadata").unwrap();

        assert_eq!(
            locate_metadata(Some(profile.to_str().unwrap())).unwrap(),
            metadata
        );
        assert_eq!(
            locate_metadata(Some(metadata.to_str().unwrap())).unwrap(),
            metadata
        );

        let _ = fs::remove_dir_all(profile);
    }

    #[test]
    #[ignore = "uses the signed-in user's read-only Kindle cache"]
    fn live_kindle_cache_parses_without_personal_output() {
        let path = locate_metadata(None).unwrap();
        let (books, excluded) = load_books(&path).unwrap();
        assert!(books.len() > 100);
        assert!(books
            .iter()
            .all(|book| !book.asin.is_empty() && !book.title.is_empty()));
        let inferred_books = books
            .iter()
            .filter(|book| book.inferred_series_name.is_some())
            .count();
        let mut groups = HashMap::new();
        for book in books
            .iter()
            .filter_map(|book| book.inferred_series_name.as_deref())
        {
            *groups.entry(book).or_insert(0usize) += 1;
        }
        let visible_series = groups.values().filter(|count| **count >= 2).count();
        println!(
            "books={}; inferred_books={inferred_books}; visible_series={visible_series}",
            books.len()
        );

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!("mmk-live-{unique}.sqlite3"));
        let database = crate::database::Database::initialize(database_path.clone()).unwrap();
        let result = database.sync(&path, &books, excluded).unwrap();
        assert_eq!(result.active_books, books.len());
        assert_eq!(database.list_books().unwrap().len(), books.len());
        drop(database);
        let _ = fs::remove_file(database_path);
    }
}
