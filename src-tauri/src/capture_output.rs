//! Windows-safe CBZ paths and atomic publication of completed backups.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const BOOK_MARKER: &str = ".mmk-asin";

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
    if manifest.format != "mmk-backup-v1"
        || manifest.pages == 0
        || archive.len() != manifest.pages as usize
    {
        return Ok(None);
    }
    for number in 1..=manifest.pages {
        let Ok(entry) = archive.by_name(&format!("screenshot_{number:05}.png")) else {
            return Ok(None);
        };
        if entry.size() == 0 || !entry.is_file() {
            return Ok(None);
        }
    }
    Ok(Some(manifest))
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
                format: "mmk-backup-v1".into(),
                asin: asin.into(),
                pages,
            })
            .map_err(|e| e.to_string())?,
        );
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for number in 1..=pages {
            guard()?;
            let name = format!("screenshot_{number:04}.png");
            let mut source = File::open(directory.join(&name)).map_err(|e| e.to_string())?;
            archive
                .start_file(format!("screenshot_{number:05}.png"), options)
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
        let _ = fs::remove_file(directory.join(format!("screenshot_{number:04}.png")));
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
