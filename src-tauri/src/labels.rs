use std::collections::HashMap;
use std::sync::OnceLock;

static LABEL_TABLE: &str = include_str!("labels.txt");

fn registry() -> &'static HashMap<String, &'static str> {
    static REGISTRY: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map = HashMap::new();
        for line in LABEL_TABLE.lines() {
            let Some((canonical, alias)) = line.split_once('\t') else {
                continue;
            };
            map.entry(normalize_key(alias)).or_insert_with(|| canonical);
        }
        map
    })
}

pub fn extract_label(title: &str) -> Option<&'static str> {
    let paren = trailing_paren_group(title)?;
    let key = normalize_key(paren);
    let registry = registry();
    if let Some(canonical) = registry.get(&key) {
        return Some(canonical);
    }
    for prefix in ["デジタル版", "デジタル"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            if let Some(canonical) = registry.get(rest) {
                return Some(canonical);
            }
        }
    }
    None
}

fn trailing_paren_group(title: &str) -> Option<&str> {
    let trimmed = title.trim_end();
    let (open, close) = if trimmed.ends_with(')') {
        ('(', ')')
    } else if trimmed.ends_with('）') {
        ('（', '）')
    } else {
        return None;
    };
    let without_close = trimmed.strip_suffix(close)?;
    let open_index = without_close.rfind(open)?;
    let content = &without_close[open_index + open.len_utf8()..];
    if content.contains(['(', ')', '（', '）']) {
        return None;
    }
    let content = content.trim();
    (!content.is_empty()).then_some(content)
}

fn normalize_key(value: &str) -> String {
    normalize_width(value)
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '・')
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_width(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(character as u32 - 0xFEE0).unwrap(),
            _ => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_labels_from_real_titles() {
        let title = "LV999の村人(1) (角川コミックス・エース)";
        assert_eq!(extract_label(title), Some("角川コミックス・エース"));

        let title = "隻眼・隻腕・隻脚の魔術師@COMIC 第1巻 (電撃コミックスNEXT)";
        assert_eq!(extract_label(title), Some("電撃コミックスNEXT"));
    }

    #[test]
    fn normalizes_width_and_digital_prefix() {
        assert_eq!(
            extract_label("サンプル 1 (デジタル版ガンガンコミックスＵＰ！)"),
            Some("ガンガンコミックスUP!")
        );
        assert_eq!(
            extract_label("サンプル 2 (ＦＬＯＳ　ＣＯＭＩＣ)"),
            Some("FLOS COMIC")
        );
    }

    #[test]
    fn maps_aliases_to_canonical_labels() {
        assert_eq!(extract_label("転生 1 (MFC)"), Some("MFコミックス"));
        assert_eq!(
            extract_label("転生 1 (エムエフコミックス　フラッパーシリーズ)"),
            Some("MFコミックス・フラッパーシリーズ")
        );
        assert_eq!(
            extract_label("転生 1 (シリウスコミックス)"),
            Some("シリウスKC")
        );
        assert_eq!(
            extract_label("転生 1 (週刊少年マガジンコミックス)"),
            Some("講談社コミックスマガジン")
        );
    }

    #[test]
    fn ignores_non_label_parentheses() {
        assert_eq!(
            extract_label("特典が付いた本 (特典: 描きおろしイラスト データ配信)"),
            None
        );
        assert_eq!(extract_label("単行本の本 (単行本)"), None);
        assert_eq!(extract_label("巻数だけの本 (4)"), None);
        assert_eq!(extract_label("括弧のない本"), None);
        assert_eq!(extract_label("マンガボックスの本 (13)"), None);
    }
}
