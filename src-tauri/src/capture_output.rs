//! Windows-safe CBZ paths and atomic publication of completed backups.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const BOOK_MARKER: &str = ".mmk-asin";

/// Page image encoding of a completed backup archive.
/// V1 archives hold PNG screenshots (web reader flow); V2 archives hold
/// JPEG screenshots (desktop app flow). Existing V1 archives stay valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveFormat {
    PngV1,
    JpegV2,
}

impl ArchiveFormat {
    pub fn manifest_format(self) -> &'static str {
        match self {
            Self::PngV1 => "mmk-backup-v1",
            Self::JpegV2 => "mmk-backup-v2",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::PngV1 => "png",
            Self::JpegV2 => "jpg",
        }
    }

    fn from_manifest(value: &str) -> Option<Self> {
        match value {
            "mmk-backup-v1" => Some(Self::PngV1),
            "mmk-backup-v2" => Some(Self::JpegV2),
            _ => None,
        }
    }

    /// Page file name inside a capture work directory (`screenshot_0001.png`).
    pub fn page_file_name(self, number: u32) -> String {
        format!("screenshot_{number:04}.{}", self.extension())
    }

    /// Entry name inside the published CBZ (`screenshot_00001.png`).
    pub fn entry_name(self, number: u32) -> String {
        format!("screenshot_{number:05}.{}", self.extension())
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct BackupManifest {
    format: String,
    asin: String,
    pages: u32,
}

fn archive_manifest(path: &Path) -> Result<Option<BackupManifest>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("CBZを確認できません: {e}")),
    };
    // Unknown or damaged files must never be overwritten or treated as completed backups.
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return Ok(None);
    };
    let Ok(manifest) = serde_json::from_slice::<BackupManifest>(archive.comment()) else {
        return Ok(None);
    };
    let Some(format) = ArchiveFormat::from_manifest(&manifest.format) else {
        return Ok(None);
    };
    if manifest.pages == 0 || archive.len() != manifest.pages as usize {
        return Ok(None);
    }
    for number in 1..=manifest.pages {
        let Ok(entry) = archive.by_name(&format.entry_name(number)) else {
            return Ok(None);
        };
        if entry.size() == 0 || !entry.is_file() {
            return Ok(None);
        }
    }
    Ok(Some(manifest))
}

/// The validated manifest of a published archive, for callers outside
/// this module (backup index scans and revalidation). Ok(None) means
/// the file is missing or is not a valid mmk archive; Err means the
/// file could not be read at all.
pub fn read_manifest(path: &Path) -> Result<Option<(String, ArchiveFormat, u32)>, String> {
    match archive_manifest(path)? {
        Some(manifest) => {
            let format = ArchiveFormat::from_manifest(&manifest.format)
                .ok_or_else(|| format!("未知のバックアップ形式です: {}", manifest.format))?;
            Ok(Some((manifest.asin, format, manifest.pages)))
        }
        None => Ok(None),
    }
}

/// (File size, mtime as unix nanoseconds) of a file: enough to
/// confirm an archive on disk is still the file an index record
/// describes, without opening the zip.
pub fn file_stamp(path: &Path) -> Result<(i64, i64), String> {
    let metadata = fs::metadata(path).map_err(|e| format!("ファイルを確認できません: {e}"))?;
    let nanos = metadata
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let mtime_nanos = i64::try_from(nanos).unwrap_or(i64::MAX);
    Ok((metadata.len() as i64, mtime_nanos))
}

/// One valid archive found by [`scan_backups`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannedBackup {
    pub asin: String,
    pub path: PathBuf,
    pub format: ArchiveFormat,
    pub pages: u32,
    pub file_size: i64,
    pub mtime_nanos: i64,
}

/// Outcome of scanning a backup root: the valid archives to index,
/// plus counts for reporting (CBZ files seen, files without a valid
/// mmk manifest). Read errors abort the scan with Err instead: a
/// partial index must never replace the stored one.
#[derive(Clone, Debug, Default)]
pub struct ScanReport {
    pub backups: Vec<ScannedBackup>,
    pub scanned_files: usize,
    pub unrecognized: usize,
}

/// Enumerate every valid mmk archive under a backup root (the root
/// itself plus its series folders) for the backup index. Dot-folders
/// such as .mmk-backup (capture work directories) are skipped, and
/// directory symlinks are never followed outside the selected root.
/// When several valid archives share an ASIN, the newest file wins.
pub fn scan_backups(root: &Path) -> Result<ScanReport, String> {
    let mut report = ScanReport::default();
    if !root.try_exists().map_err(|e| e.to_string())? {
        return Ok(report);
    }
    let mut directories = vec![root.to_path_buf()];
    for entry in fs::read_dir(root).map_err(|e| format!("保存先を確認できません: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        // DirEntry::file_type does not follow symlinks, so a linked
        // directory reports as a symlink, not a dir, and is skipped.
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        directories.push(entry.path());
    }
    let mut found: std::collections::HashMap<String, ScannedBackup> =
        std::collections::HashMap::new();
    for directory in directories {
        for entry in
            fs::read_dir(&directory).map_err(|e| format!("保存先を確認できません: {e}"))?
        {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cbz"))
            {
                continue;
            }
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                continue;
            }
            report.scanned_files += 1;
            let Some((asin, format, pages)) = read_manifest(&path)? else {
                report.unrecognized += 1;
                continue;
            };
            let asin = asin.to_uppercase();
            if super::validate_asin(&asin).is_err() {
                report.unrecognized += 1;
                continue;
            }
            let (file_size, mtime_nanos) = file_stamp(&path)?;
            let scanned = ScannedBackup {
                asin: asin.clone(),
                path,
                format,
                pages,
                file_size,
                mtime_nanos,
            };
            // Newest wins; ties break deterministically by size,
            // then by path, so the choice never depends on read
            // order.
            let replace = match found.get(&asin) {
                None => true,
                Some(existing) => {
                    (mtime_nanos, file_size) > (existing.mtime_nanos, existing.file_size)
                        || ((mtime_nanos, file_size)
                            == (existing.mtime_nanos, existing.file_size)
                            && scanned.path < existing.path)
                }
            };
            if replace {
                found.insert(asin, scanned);
            }
        }
    }
    report.backups = found.into_values().collect();
    Ok(report)
}

pub fn archive_path(
    root: &Path,
    series: Option<&str>,
    title: &str,
    asin: &str,
) -> Result<PathBuf, String> {
    super::validate_asin(asin)?;
    let parent = root.join(folder_name(series.unwrap_or(""), "シリーズなし"));
    let title = folder_name(title, "書籍名なし");
    let plain = parent.join(format!("{title}.cbz"));
    if !plain.try_exists().map_err(|e| e.to_string())?
        || archive_manifest(&plain)?.is_some_and(|m| m.asin.eq_ignore_ascii_case(asin))
    {
        return Ok(plain);
    }
    let distinct = parent.join(format!("{title} [{asin}].cbz"));
    if distinct.try_exists().map_err(|e| e.to_string())?
        && !archive_manifest(&distinct)?.is_some_and(|m| m.asin.eq_ignore_ascii_case(asin))
    {
        return Err(format!(
            "同名のCBZが存在し、安全に保存できません: {}",
            distinct.display()
        ));
    }
    Ok(distinct)
}

pub fn has_backup(path: &Path, asin: &str) -> Result<bool, String> {
    Ok(archive_manifest(path)?.is_some_and(|m| m.asin.eq_ignore_ascii_case(asin)))
}

pub fn work_directory(root: &Path, asin: &str) -> Result<PathBuf, String> {
    super::validate_asin(asin)?;
    let directory = root
        .join(".mmk-backup")
        .join(asin)
        .join(uuid::Uuid::new_v4().to_string());
    prepare_directory(&directory, asin)?;
    Ok(directory)
}

pub fn finish_archive(
    directory: &Path,
    destination: &Path,
    asin: &str,
    pages: u32,
    guard: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    finish_archive_with_format(
        directory,
        destination,
        asin,
        pages,
        ArchiveFormat::PngV1,
        guard,
    )
}

/// Verify that a work directory holds exactly the contiguous, non-empty
/// page files a completed capture must have produced.
pub fn verify_pages(directory: &Path, pages: u32, format: ArchiveFormat) -> Result<(), String> {
    if pages == 0 {
        return Err("保存画像がないためバックアップを確定できません".into());
    }
    for number in 1..=pages {
        let path = directory.join(format.page_file_name(number));
        let metadata = fs::metadata(&path)
            .map_err(|e| format!("ページ画像を確認できません ({}): {e}", path.display()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(format!("ページ画像が空です: {}", path.display()));
        }
    }
    Ok(())
}

pub fn finish_archive_with_format(
    directory: &Path,
    destination: &Path,
    asin: &str,
    pages: u32,
    format: ArchiveFormat,
    mut guard: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    if pages == 0 {
        return Err("保存画像がないためCBZを生成できません".into());
    }
    let parent = destination.parent().ok_or("CBZの保存先が不正です")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".mmk-{}.tmp", uuid::Uuid::new_v4()));
    let result: Result<(), String> = (|| {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let mut archive = zip::ZipWriter::new(file);
        archive.set_comment(
            serde_json::to_string(&BackupManifest {
                format: format.manifest_format().into(),
                asin: asin.into(),
                pages,
            })
            .map_err(|e| e.to_string())?,
        );
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for number in 1..=pages {
            guard()?;
            let name = format.page_file_name(number);
            let mut source = File::open(directory.join(&name)).map_err(|e| e.to_string())?;
            archive
                .start_file(format.entry_name(number), options)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut archive).map_err(|e| e.to_string())?;
        }
        archive
            .finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        guard()?;
        publish_without_overwrite(&temporary, destination)
            .map_err(|e| format!("CBZを確定できません（既存ファイルは上書きしません）: {e}"))?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result?;
    // Remove only this run's known files after the complete CBZ has been published.
    for number in 1..=pages {
        let _ = fs::remove_file(directory.join(format.page_file_name(number)));
    }
    let _ = fs::remove_file(directory.join(BOOK_MARKER));
    let _ = fs::remove_dir(directory);
    Ok(())
}

#[cfg(windows)]
fn publish_without_overwrite(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // No MOVEFILE_REPLACE_EXISTING: also works on FAT/exFAT without hard links.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn publish_without_overwrite(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::hard_link(source, destination)
}

fn folder_name(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        return fallback.into();
    }
    let cleaned: String = value
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_end_matches([' ', '.']);
    let reserved = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(
        reserved.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || reserved
        .strip_prefix("COM")
        .or_else(|| reserved.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            (suffix.len() == 1 && (b'1'..=b'9').contains(&suffix.as_bytes()[0]))
                || matches!(suffix, "¹" | "²" | "³")
        });
    let mut units = 0;
    let shortened: String = cleaned
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 80
        })
        .collect();
    if shortened == value && !reserved && !shortened.is_empty() {
        return shortened;
    }
    // A stable suffix prevents sanitized/truncated names from merging series.
    let hash = format!("{:x}", md5::compute(value.as_bytes()));
    let name = if shortened.is_empty() {
        fallback
    } else {
        shortened.as_str()
    };
    format!("{name}~{}", &hash[..12])
}

fn folder_asin(directory: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(directory.join(BOOK_MARKER)) {
        Ok(value) => {
            let asin = value.trim();
            super::validate_asin(asin)
                .map_err(|_| format!("保存先の書籍識別情報が不正です: {}", directory.display()))?;
            Ok(Some(asin.to_owned()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("保存先の書籍識別情報を確認できません: {error}")),
    }
}

#[cfg(test)]
pub fn book_directory(
    root: &Path,
    series: Option<&str>,
    title: &str,
    asin: &str,
) -> Result<PathBuf, String> {
    super::validate_asin(asin)?;
    let series = folder_name(series.unwrap_or(""), "シリーズなし");
    let title = folder_name(title, "書籍名なし");
    let directory = root.join(series).join(&title);
    match folder_asin(&directory)? {
        Some(owner) if !owner.eq_ignore_ascii_case(asin) => {}
        _ => return Ok(directory),
    }
    // Use the plain book title normally; disambiguate only an actual name collision.
    let directory = directory.with_file_name(format!("{title} [{asin}]"));
    if folder_asin(&directory)?.is_some_and(|owner| !owner.eq_ignore_ascii_case(asin)) {
        return Err(format!(
            "保存先が別の書籍に使われています: {}",
            directory.display()
        ));
    }
    Ok(directory)
}

pub fn prepare_directory(directory: &Path, asin: &str) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|e| format!("保存先を作成できません: {e}"))?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(BOOK_MARKER))
    {
        Ok(mut file) => file
            .write_all(asin.as_bytes())
            .map_err(|e| format!("書籍識別情報を保存できません: {e}")),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if folder_asin(directory)?.is_some_and(|owner| owner.eq_ignore_ascii_case(asin)) {
                Ok(())
            } else {
                Err("保存先の書籍識別情報が一致しません".into())
            }
        }
        Err(error) => Err(format!("書籍識別情報を保存できません: {error}")),
    }
}

#[cfg(test)]
pub fn next_screenshot_number(directory: &Path) -> Result<u64, String> {
    let mut maximum = 0;
    for entry in fs::read_dir(directory).map_err(|e| format!("保存先を確認できません: {e}"))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Some(number) = name
            .strip_prefix("screenshot_")
            .and_then(|name| name.strip_suffix(".png"))
        else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        maximum = maximum.max(
            number
                .parse::<u64>()
                .map_err(|_| "既存画像の番号が大きすぎます")?,
        );
    }
    maximum
        .checked_add(1)
        .ok_or_else(|| "画像の連番が上限に達しました".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_backups_indexes_valid_archives_and_counts_the_rest() {
        let root = std::env::temp_dir().join(format!("mmk-scan-{}", uuid::Uuid::new_v4()));
        // A valid archive inside a series folder.
        let work = work_directory(&root, "B000000001").unwrap();
        fs::write(work.join("screenshot_0001.png"), b"page").unwrap();
        let destination = archive_path(&root, Some("シリーズ"), "書籍", "B000000001").unwrap();
        finish_archive(&work, &destination, "B000000001", 1, || Ok(())).unwrap();
        // A valid archive directly under the root.
        let work = work_directory(&root, "B000000002").unwrap();
        fs::write(work.join("screenshot_0001.png"), b"page").unwrap();
        let loose = root.join("単独.cbz");
        finish_archive(&work, &loose, "B000000002", 1, || Ok(())).unwrap();
        // A .cbz without an mmk manifest (legacy library entry).
        fs::write(root.join("旧ライブラリ.cbz"), b"not a zip").unwrap();
        // A valid archive under .mmk-backup must not be indexed.
        let hidden = root.join(".mmk-backup").join("隠し.cbz");
        let work = work_directory(&root, "B000000003").unwrap();
        fs::write(work.join("screenshot_0001.png"), b"page").unwrap();
        finish_archive(&work, &hidden, "B000000003", 1, || Ok(())).unwrap();

        let report = scan_backups(&root).unwrap();
        assert_eq!(report.scanned_files, 3);
        assert_eq!(report.unrecognized, 1);
        let asins: Vec<&str> = report
            .backups
            .iter()
            .map(|backup| backup.asin.as_str())
            .collect();
        assert_eq!(asins.len(), 2);
        assert!(asins.contains(&"B000000001"));
        assert!(asins.contains(&"B000000002"));
        let first = report
            .backups
            .iter()
            .find(|backup| backup.asin == "B000000001")
            .unwrap();
        assert_eq!(first.path, destination);
        assert_eq!(first.pages, 1);
        assert!(first.file_size > 0);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cbz_round_trip_preserves_page_order_and_disambiguates_same_title() {
        let root = std::env::temp_dir().join(format!("mmk-cbz-{}", uuid::Uuid::new_v4()));
        let work = work_directory(&root, "B000000001").unwrap();
        for (number, contents) in [
            (1, b"first page".as_slice()),
            (2, b"second page".as_slice()),
        ] {
            fs::write(work.join(format!("screenshot_{number:04}.png")), contents).unwrap();
        }
        let destination = archive_path(&root, Some("シリーズ"), "書籍.vol1", "B000000001").unwrap();
        assert!(destination.ends_with("シリーズ/書籍.vol1.cbz"));
        finish_archive(&work, &destination, "B000000001", 2, || Ok(())).unwrap();
        assert!(!work.exists());
        assert!(has_backup(&destination, "B000000001").unwrap());
        assert!(!has_backup(&destination, "B000000002").unwrap());
        let mut archive = zip::ZipArchive::new(File::open(&destination).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        for (index, expected) in [b"first page".as_slice(), b"second page".as_slice()]
            .into_iter()
            .enumerate()
        {
            let mut entry = archive.by_index(index).unwrap();
            assert_eq!(entry.name(), format!("screenshot_{:05}.png", index + 1));
            let mut contents = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut contents).unwrap();
            assert_eq!(contents, expected);
        }
        drop(archive);
        assert!(
            archive_path(&root, Some("シリーズ"), "書籍.vol1", "B000000002")
                .unwrap()
                .ends_with("書籍.vol1 [B000000002].cbz")
        );
        fs::remove_file(destination).unwrap();
        fs::remove_dir(root.join("シリーズ")).unwrap();
        fs::remove_dir(root.join(".mmk-backup/B000000001")).unwrap();
        fs::remove_dir(root.join(".mmk-backup")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn jpeg_v2_round_trip_and_page_verification() {
        let root = std::env::temp_dir().join(format!("mmk-cbz-v2-{}", uuid::Uuid::new_v4()));
        let work = work_directory(&root, "B000000001").unwrap();
        for (number, contents) in [(1, b"jpeg one".as_slice()), (2, b"jpeg two".as_slice())] {
            fs::write(
                work.join(ArchiveFormat::JpegV2.page_file_name(number)),
                contents,
            )
            .unwrap();
        }
        assert!(verify_pages(&work, 2, ArchiveFormat::JpegV2).is_ok());
        assert!(verify_pages(&work, 3, ArchiveFormat::JpegV2).is_err());
        assert!(verify_pages(&work, 2, ArchiveFormat::PngV1).is_err());
        let destination = archive_path(&root, None, "書籍", "B000000001").unwrap();
        finish_archive_with_format(
            &work,
            &destination,
            "B000000001",
            2,
            ArchiveFormat::JpegV2,
            || Ok(()),
        )
        .unwrap();
        assert!(!work.exists());
        assert!(has_backup(&destination, "B000000001").unwrap());
        let mut archive = zip::ZipArchive::new(File::open(&destination).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        let manifest: BackupManifest =
            serde_json::from_slice(archive.comment()).unwrap();
        assert_eq!(manifest.format, "mmk-backup-v2");
        let entry = archive.by_index(0).unwrap();
        assert_eq!(entry.name(), "screenshot_00001.jpg");
        drop(entry);
        drop(archive);
        fs::remove_file(destination).unwrap();
        fs::remove_dir(root.join("シリーズなし")).unwrap();
        fs::remove_dir(root.join(".mmk-backup/B000000001")).unwrap();
        fs::remove_dir(root.join(".mmk-backup")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn cancelled_failed_and_existing_outputs_never_publish_partial_cbz_or_remove_pages() {
        let root = std::env::temp_dir().join(format!("mmk-cbz-failure-{}", uuid::Uuid::new_v4()));
        let work = work_directory(&root, "B000000001").unwrap();
        let page = work.join("screenshot_0001.png");
        fs::write(&page, b"original page").unwrap();
        let destination = root.join("backup.cbz");
        assert!(finish_archive(&work, &destination, "B000000001", 1, || Err(
            "cancelled".into()
        ))
        .is_err());
        assert!(!destination.exists());
        assert!(finish_archive(&work, &destination, "B000000001", 2, || Ok(())).is_err());
        assert!(!destination.exists());
        fs::write(&destination, b"existing backup").unwrap();
        assert!(finish_archive(&work, &destination, "B000000001", 1, || Ok(())).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"existing backup");
        assert_eq!(fs::read(&page).unwrap(), b"original page");
        assert!(!has_backup(&destination, "B000000001").unwrap());
        assert!(!fs::read_dir(&root).unwrap().any(|e| e
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "tmp")));
        fs::remove_file(page).unwrap();
        fs::remove_file(work.join(BOOK_MARKER)).unwrap();
        fs::remove_dir(work).unwrap();
        fs::remove_file(destination).unwrap();
        fs::remove_dir(root.join(".mmk-backup/B000000001")).unwrap();
        fs::remove_dir(root.join(".mmk-backup")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn keeps_readable_names_and_groups_unassigned_books() {
        let root = Path::new("output");
        assert_eq!(
            book_directory(root, Some("シリーズA"), "本（１）", "B000000001").unwrap(),
            root.join("シリーズA").join("本（１）")
        );
        for series in [None, Some(""), Some(" \t ")] {
            assert_eq!(
                book_directory(root, series, "単巻", "B000000001").unwrap(),
                root.join("シリーズなし").join("単巻")
            );
        }
    }

    #[test]
    fn prevents_traversal_invalid_windows_names_and_truncation_collisions() {
        for value in [
            "../escape",
            "..",
            ".",
            "CON",
            "NUL.txt",
            "COM1",
            "LPT¹",
            "trailing. ",
            "bad:name",
            "bad\u{0000}name",
        ] {
            let name = folder_name(value, "fallback");
            assert!(!name.is_empty());
            assert_eq!(Path::new(&name).components().count(), 1);
            assert!(!name
                .chars()
                .any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c)));
            assert!(!name.ends_with([' ', '.']));
        }
        assert_ne!(
            folder_name("A/B", "fallback"),
            folder_name("A_B", "fallback")
        );
        let long = "😀".repeat(100);
        let name = folder_name(&long, "fallback");
        assert!(name.encode_utf16().count() <= 93);
        assert_ne!(name, folder_name(&(long + "2"), "fallback"));
        assert!(book_directory(Path::new("output"), None, "title", "../../bad").is_err());
    }

    #[test]
    fn disambiguates_identical_book_titles_without_overwriting_markers() {
        let root = std::env::temp_dir().join(format!("mmk-output-{}", uuid::Uuid::new_v4()));
        let first = book_directory(&root, None, "同名の本", "B000000001").unwrap();
        prepare_directory(&first, "B000000001").unwrap();
        assert_eq!(
            book_directory(&root, None, "同名の本", "B000000001").unwrap(),
            first
        );
        let second = book_directory(&root, None, "同名の本", "B000000002").unwrap();
        assert_ne!(first, second);
        assert!(second.ends_with("同名の本 [B000000002]"));
        prepare_directory(&second, "B000000002").unwrap();
        assert!(prepare_directory(&first, "B000000002").is_err());
        fs::remove_file(first.join(BOOK_MARKER)).unwrap();
        fs::remove_dir(&first).unwrap();
        fs::remove_file(second.join(BOOK_MARKER)).unwrap();
        fs::remove_dir(&second).unwrap();
        fs::remove_dir(root.join("シリーズなし")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn numbering_continues_after_existing_files_and_reserved_names() {
        let root = std::env::temp_dir().join(format!("mmk-numbering-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        assert_eq!(next_screenshot_number(&root).unwrap(), 1);
        for name in [
            "screenshot_0001.png",
            "SCREENSHOT_0020.PNG",
            "screenshot_other.png",
        ] {
            fs::write(root.join(name), b"existing").unwrap();
        }
        assert_eq!(next_screenshot_number(&root).unwrap(), 21);
        fs::write(
            root.join("screenshot_18446744073709551615.png"),
            b"existing",
        )
        .unwrap();
        assert!(next_screenshot_number(&root).is_err());
        for entry in fs::read_dir(&root).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}

