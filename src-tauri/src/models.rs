use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ImportedBook {
    pub asin: String,
    pub title: String,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub acquisition_date: Option<String>,
    pub content_type: Option<String>,
    pub ownership_type: String,
    pub source_origin: String,
    pub cover_path: Option<String>,
    pub inferred_series_name: Option<String>,
    pub inferred_series_index: Option<f64>,
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookView {
    pub id: i64,
    pub asin: String,
    pub title: String,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub acquisition_date: Option<String>,
    pub ownership_type: String,
    pub source_origin: String,
    pub source_status: String,
    pub cover_path: Option<String>,
    pub score: i64,
    pub favorite: bool,
    pub series_mode: String,
    pub inferred_series_name: Option<String>,
    pub inferred_series_index: Option<f64>,
    pub manual_series_name: Option<String>,
    pub manual_series_index: Option<f64>,
    pub effective_series_name: Option<String>,
    pub effective_series_index: Option<f64>,
    pub manual_title: Option<String>,
    pub manual_authors: Option<String>,
    pub manual_publisher: Option<String>,
    pub label: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadataInput {
    pub book_id: i64,
    pub score: i64,
    pub favorite: bool,
    pub series_mode: String,
    pub manual_series_name: Option<String>,
    pub manual_series_index: Option<f64>,
    pub manual_title: Option<String>,
    pub manual_authors: Option<String>,
    pub manual_publisher: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesMetadataInput {
    pub book_ids: Vec<i64>,
    pub score_delta: i64,
    pub favorite: Option<bool>,
    pub rename_to: Option<String>,
    pub tags_to_add: Vec<String>,
    pub tags_to_remove: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub source_path: String,
    pub active_books: usize,
    pub inserted_books: usize,
    pub updated_books: usize,
    pub excluded_books: usize,
    pub missing_books: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonContinueSeriesItemInput {
    pub asin: String,
    pub title: String,
    pub price: Option<i64>,
    pub image_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonContinueSeriesItemView {
    pub asin: String,
    pub title: String,
    pub product_url: String,
    pub price: Option<i64>,
    pub image_url: Option<String>,
    pub series_name: Option<String>,
    pub volume_index: Option<f64>,
    pub seen_at: String,
}
