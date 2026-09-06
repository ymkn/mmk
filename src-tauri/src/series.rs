use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    High,
    Contextual,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeriesCandidate {
    pub display_name: String,
    pub match_key: String,
    pub volume_index: f64,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InferredSeries {
    pub name: String,
    pub volume_index: f64,
}

pub fn infer_candidate(title: &str) -> Option<SeriesCandidate> {
    let normalized = normalize_width(title);
    let title = normalized.trim();
    if title.is_empty() {
        return None;
    }

    if let Some(candidate) = infer_suffix_candidate(title) {
        return Some(candidate);
    }

    let without_qualifiers = strip_trailing_qualifiers(title);
    if without_qualifiers != title {
        if let Some(candidate) = infer_suffix_candidate(without_qualifiers) {
            return Some(candidate);
        }
    }

    contextual_number_template(without_qualifiers)
}

fn infer_suffix_candidate(title: &str) -> Option<SeriesCandidate> {
    if let Some(candidate) = parenthesized_number(title) {
        return Some(candidate);
    }
    if let Some(candidate) = japanese_volume(title) {
        return Some(candidate);
    }
    if let Some(candidate) = named_volume(title) {
        return Some(candidate);
    }
    if let Some(candidate) = hash_volume(title) {
        return Some(candidate);
    }
    if let Some(candidate) = roman_volume(title) {
        return Some(candidate);
    }
    if let Some(candidate) = japanese_volume_with_subtitle(title) {
        return Some(candidate);
    }
    if let Some(candidate) = parenthesized_number_with_suffix(title) {
        return Some(candidate);
    }
    trailing_number(title)
}

pub fn infer_library(titles: &[String]) -> Vec<Option<InferredSeries>> {
    let candidates: Vec<_> = titles.iter().map(|title| infer_candidate(title)).collect();
    let mut contextual_volumes: HashMap<String, HashSet<u64>> = HashMap::new();

    for candidate in candidates.iter().flatten() {
        if candidate.confidence == Confidence::Contextual {
            contextual_volumes
                .entry(candidate.match_key.clone())
                .or_default()
                .insert(candidate.volume_index.to_bits());
        }
    }

    candidates
        .into_iter()
        .map(|candidate| {
            candidate.and_then(|candidate| {
                let accepted = candidate.confidence == Confidence::High
                    || contextual_volumes
                        .get(&candidate.match_key)
                        .is_some_and(|volumes| volumes.len() >= 2)
                    || candidate.volume_index < 100.0;
                accepted.then_some(InferredSeries {
                    name: candidate.display_name,
                    volume_index: candidate.volume_index,
                })
            })
        })
        .collect()
}

fn make_candidate(name: &str, number: &str, confidence: Confidence) -> Option<SeriesCandidate> {
    let display_name = name.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '-' | '‐' | '–' | '—' | ':' | '：')
    });
    if display_name.is_empty() {
        return None;
    }
    let volume_index = number.parse::<f64>().ok()?;
    Some(SeriesCandidate {
        display_name: display_name.to_owned(),
        match_key: canonical_key(display_name),
        volume_index,
        confidence,
    })
}

fn parenthesized_number(title: &str) -> Option<SeriesCandidate> {
    let (open, close) = if title.ends_with(')') {
        ('(', ')')
    } else if title.ends_with('）') {
        ('（', '）')
    } else {
        return None;
    };
    let without_close = title.strip_suffix(close)?;
    let open_index = without_close.rfind(open)?;
    let number = without_close[open_index + open.len_utf8()..].trim();
    is_decimal(number).then_some(())?;
    make_candidate(&without_close[..open_index], number, Confidence::High)
}

fn japanese_volume(title: &str) -> Option<SeriesCandidate> {
    let without_marker = title.strip_suffix('巻')?.trim_end();
    let (name, number) = split_trailing_decimal(without_marker)?;
    let name = name
        .trim_end()
        .strip_suffix('第')
        .unwrap_or(name)
        .trim_end();
    make_candidate(name, number, Confidence::High)
}

fn named_volume(title: &str) -> Option<SeriesCandidate> {
    let (before_number, number) = split_trailing_decimal(title)?;
    let before_number = before_number.trim_end();
    let lower = before_number.to_ascii_lowercase();
    for marker in ["volume", "vol.", "vol"] {
        if lower.ends_with(marker) {
            let name = &before_number[..before_number.len() - marker.len()];
            return make_candidate(name, number, Confidence::High);
        }
    }
    None
}

fn hash_volume(title: &str) -> Option<SeriesCandidate> {
    let (before_number, number) = split_trailing_decimal(title)?;
    let name = before_number.trim_end().strip_suffix('#')?;
    make_candidate(name, number, Confidence::High)
}

fn roman_volume(title: &str) -> Option<SeriesCandidate> {
    let run_start = title
        .char_indices()
        .rev()
        .take_while(|(_, character)| matches!(character, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
        .last()
        .map(|(index, _)| index)?;
    let numeral = &title[run_start..];
    if numeral.chars().count() < 2 && numeral != "I" {
        return None;
    }
    let volume = parse_roman(numeral)?;
    let name = title[..run_start].trim_end();
    if name.is_empty() {
        return None;
    }
    let preceding = name.chars().next_back();
    if preceding.is_some_and(|character| character.is_ascii_alphanumeric()) {
        return None;
    }
    make_candidate(name, &volume.to_string(), Confidence::Contextual)
}

fn parse_roman(value: &str) -> Option<u32> {
    const TABLE: &[(&str, u32)] = &[
        ("MMM", 3000),
        ("MM", 2000),
        ("M", 1000),
        ("CM", 900),
        ("CD", 400),
        ("DCCC", 800),
        ("DCC", 700),
        ("DC", 600),
        ("D", 500),
        ("CCC", 300),
        ("CC", 200),
        ("C", 100),
        ("XC", 90),
        ("XL", 40),
        ("LXXX", 80),
        ("LXX", 70),
        ("LX", 60),
        ("L", 50),
        ("XXX", 30),
        ("XX", 20),
        ("X", 10),
        ("IX", 9),
        ("IV", 4),
        ("VIII", 8),
        ("VII", 7),
        ("VI", 6),
        ("V", 5),
        ("III", 3),
        ("II", 2),
        ("I", 1),
    ];
    let mut rest = value;
    let mut total = 0;
    let mut last = u32::MAX;
    while !rest.is_empty() {
        let Some((token, token_value)) = TABLE.iter().find(|(token, _)| rest.starts_with(token))
        else {
            return None;
        };
        if *token_value >= last {
            return None;
        }
        last = *token_value;
        total += token_value;
        rest = &rest[token.len()..];
    }
    Some(total)
}

const SUBTITLE_OPENERS: &[char] = &['~', '～', '(', '（', '[', '［', '【', '「', '『', ':', '：'];

fn japanese_volume_with_subtitle(title: &str) -> Option<SeriesCandidate> {
    let mut search_from = 0;
    while let Some(offset) = title[search_from..].find('巻') {
        let marker_start = search_from + offset;
        let marker_end = marker_start + '巻'.len_utf8();
        search_from = marker_end;
        let before_marker = &title[..marker_start];
        let Some((number_start, _)) = before_marker
            .char_indices()
            .rev()
            .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
            .last()
        else {
            continue;
        };
        let number = &before_marker[number_start..];
        if !is_decimal(number) {
            continue;
        }
        let before_number = &before_marker[..number_start];
        let name_end = if before_number.ends_with('第') {
            number_start - '第'.len_utf8()
        } else if before_number
            .chars()
            .next_back()
            .is_some_and(is_bare_volume_boundary)
        {
            number_start
        } else {
            continue;
        };
        let after = title[marker_end..].trim_start();
        if !after
            .chars()
            .next()
            .is_some_and(|character| SUBTITLE_OPENERS.contains(&character))
        {
            continue;
        }
        if let Some(candidate) = make_candidate(&title[..name_end], number, Confidence::High) {
            return Some(candidate);
        }
    }
    None
}

fn is_bare_volume_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '!' | '?' | '。' | '～' | '~' | '）' | ')' | '］' | ']' | '】' | '」' | '』'
        )
}

fn parenthesized_number_with_suffix(title: &str) -> Option<SeriesCandidate> {
    for (open, close) in [('(', ')'), ('（', '）')] {
        let mut search_from = 0;
        while let Some(offset) = title[search_from..].find(open) {
            let open_index = search_from + offset;
            search_from = open_index + open.len_utf8();
            let rest = &title[search_from..];
            let number_len = rest
                .char_indices()
                .find(|(_, character)| !(character.is_ascii_digit() || *character == '.'))
                .map(|(index, _)| index)
                .unwrap_or(rest.len());
            let number = &rest[..number_len];
            if !is_decimal(number) {
                continue;
            }
            let Some(after_number) = rest[number_len..].strip_prefix(close) else {
                continue;
            };
            if after_number.trim().is_empty() {
                continue;
            }
            if let Some(candidate) = make_candidate(
                strip_trailing_qualifiers(&title[..open_index]),
                number,
                Confidence::High,
            ) {
                return Some(candidate);
            }
        }
    }
    None
}

fn trailing_number(title: &str) -> Option<SeriesCandidate> {
    let (name, number) = split_trailing_decimal(title)?;
    if !has_volume_number_boundary(name) {
        return None;
    }
    make_candidate(
        strip_trailing_qualifiers(name),
        number,
        Confidence::Contextual,
    )
}

fn has_volume_number_boundary(name: &str) -> bool {
    let Some(last) = name.chars().next_back() else {
        return false;
    };
    if last.is_whitespace() || matches!(last, '!' | '?' | '。') {
        return true;
    }
    if last.is_ascii_alphabetic() {
        let letters: Vec<char> = name
            .chars()
            .rev()
            .take_while(|character| character.is_ascii_alphabetic())
            .collect();
        return letters.len() >= 2
            && letters
                .iter()
                .all(|character| character.is_ascii_uppercase());
    }
    false
}

fn contextual_number_template(title: &str) -> Option<SeriesCandidate> {
    let mut candidates = Vec::new();
    let mut start = None;
    for (index, character) in title.char_indices() {
        if character.is_ascii_digit() {
            start.get_or_insert(index);
            continue;
        }
        if character == '.' && start.is_some() {
            continue;
        }
        if let Some(number_start) = start.take() {
            candidates.push((number_start, index));
        }
    }
    if let Some(number_start) = start {
        candidates.push((number_start, title.len()));
    }

    for (number_start, number_end) in candidates.into_iter().rev() {
        let before = &title[..number_start];
        let after = &title[number_end..];
        if before.trim().is_empty() || after.trim().is_empty() {
            continue;
        }
        if !is_token_boundary(before.chars().next_back())
            || !is_token_boundary(after.chars().next())
        {
            continue;
        }
        let number = &title[number_start..number_end];
        if !is_decimal(number) {
            continue;
        }

        let prefix = before.trim_end_matches(is_series_separator);
        let suffix = after.trim_matches(is_series_separator);
        if prefix.is_empty() || suffix.is_empty() {
            continue;
        }
        let display_name = format!("{} {}", prefix.trim_end(), suffix.trim_start());
        let display_name = display_name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        return Some(SeriesCandidate {
            match_key: canonical_key(&display_name),
            display_name,
            volume_index: number.parse().ok()?,
            confidence: Confidence::Contextual,
        });
    }
    None
}

fn is_token_boundary(character: Option<char>) -> bool {
    character.is_none_or(|character| {
        character.is_whitespace()
            || matches!(
                character,
                ':' | '：' | '-' | '‐' | '–' | '—' | '~' | '～' | '(' | '（' | '[' | '［' | '【'
            )
    })
}

fn is_series_separator(character: char) -> bool {
    character.is_whitespace()
        || matches!(character, ':' | '：' | '-' | '‐' | '–' | '—' | '~' | '～')
}

fn strip_trailing_qualifiers(mut value: &str) -> &str {
    loop {
        let trimmed = value.trim_end();
        let Some(close) = trimmed.chars().next_back() else {
            return trimmed;
        };
        let open = match close {
            ')' => '(',
            '）' => '（',
            ']' => '[',
            '］' => '［',
            '】' => '【',
            _ => return trimmed,
        };
        let without_close = &trimmed[..trimmed.len() - close.len_utf8()];
        let Some(open_index) = without_close.rfind(open) else {
            return trimmed;
        };
        let content = without_close[open_index + open.len_utf8()..].trim();
        if content.is_empty() || is_decimal(content) {
            return trimmed;
        }
        value = &without_close[..open_index];
    }
}

fn split_trailing_decimal(value: &str) -> Option<(&str, &str)> {
    let end = value.len();
    let start = value
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
        .last()
        .map(|(index, _)| index)?;
    let number = &value[start..end];
    is_decimal(number).then_some((&value[..start], number))
}

fn is_decimal(value: &str) -> bool {
    let mut dots = 0;
    let mut digits = 0;
    for character in value.chars() {
        if character == '.' {
            dots += 1;
        } else if character.is_ascii_digit() {
            digits += 1;
        } else {
            return false;
        }
    }
    digits > 0 && dots <= 1 && !value.starts_with('.') && !value.ends_with('.')
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

fn canonical_key(value: &str) -> String {
    normalize_width(value)
        .chars()
        .filter(|character| {
            !character.is_whitespace()
                && !matches!(character, '-' | '‐' | '–' | '—' | '_' | ':' | '：' | '・')
        })
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_volume_markers_are_high_confidence() {
        for (title, expected_name, expected_volume) in [
            ("星巡り 第3巻", "星巡り", 3.0),
            ("星巡り 3巻", "星巡り", 3.0),
            ("星巡り Vol. 3", "星巡り", 3.0),
            ("星巡り Volume 3", "星巡り", 3.0),
            ("星巡り (3)", "星巡り", 3.0),
            ("星巡り（３）", "星巡り", 3.0),
            ("星巡り #3", "星巡り", 3.0),
        ] {
            let candidate = infer_candidate(title).unwrap();
            assert_eq!(candidate.display_name, expected_name);
            assert_eq!(candidate.volume_index, expected_volume);
            assert_eq!(candidate.confidence, Confidence::High);
        }
    }

    #[test]
    fn bare_number_requires_multiple_distinct_volumes() {
        let titles = vec![
            "星巡り 1".to_owned(),
            "星巡り 2".to_owned(),
            "単発資料 2026".to_owned(),
        ];
        let inferred = infer_library(&titles);
        assert_eq!(inferred[0].as_ref().unwrap().name, "星巡り");
        assert_eq!(inferred[1].as_ref().unwrap().volume_index, 2.0);
        assert_eq!(inferred[2], None);
    }

    #[test]
    fn trailing_imprint_does_not_hide_explicit_or_contextual_volume() {
        let explicit = infer_candidate("星巡り 6巻 (青空コミックス)").unwrap();
        assert_eq!(explicit.display_name, "星巡り");
        assert_eq!(explicit.volume_index, 6.0);
        assert_eq!(explicit.confidence, Confidence::High);

        let titles = vec![
            "星巡り 1 (青空コミックス)".to_owned(),
            "星巡り 2 (青空コミックス)".to_owned(),
        ];
        let inferred = infer_library(&titles);
        assert_eq!(inferred[0].as_ref().unwrap().name, "星巡り");
        assert_eq!(inferred[1].as_ref().unwrap().volume_index, 2.0);
    }

    #[test]
    fn number_before_subtitle_requires_matching_library_context() {
        let titles = vec![
            "星巡り 1 ～北の章～ (青空コミックス)".to_owned(),
            "星巡り 2 ～北の章～ (青空コミックス)".to_owned(),
            "単発資料 2026 年版".to_owned(),
        ];
        let inferred = infer_library(&titles);
        assert_eq!(inferred[0].as_ref().unwrap().name, "星巡り 北の章");
        assert_eq!(inferred[1].as_ref().unwrap().volume_index, 2.0);
        assert_eq!(inferred[2], None);
    }

    #[test]
    fn multiple_trailing_qualifiers_are_ignored() {
        let candidate = infer_candidate("星巡り: 5【イラスト特典付】 (青空コミックス)").unwrap();
        assert_eq!(candidate.display_name, "星巡り");
        assert_eq!(candidate.volume_index, 5.0);
        assert_eq!(candidate.confidence, Confidence::Contextual);
    }

    #[test]
    fn equal_contextual_volumes_do_not_form_a_series() {
        let titles = vec!["資料 2026".to_owned(), "資料 2026".to_owned()];
        assert_eq!(infer_library(&titles), vec![None, None]);
    }

    #[test]
    fn unrelated_title_is_not_a_candidate() {
        assert_eq!(infer_candidate("巻数のない本"), None);
    }

    #[test]
    fn mid_title_volume_marker_with_subtitle_is_high_confidence() {
        let candidate = infer_candidate(
            "隻眼・隻腕・隻脚の魔術師@COMIC 第1巻～森の小屋に籠っていたら早2000年。気づけば魔神と呼ばれていた。僕はただ魔術の探求をしたいだけなのに～",
        )
        .unwrap();
        assert_eq!(candidate.display_name, "隻眼・隻腕・隻脚の魔術師@COMIC");
        assert_eq!(candidate.volume_index, 1.0);
        assert_eq!(candidate.confidence, Confidence::High);

        let short = infer_candidate("星巡り 2巻～副題～").unwrap();
        assert_eq!(short.display_name, "星巡り");
        assert_eq!(short.volume_index, 2.0);
        assert_eq!(short.confidence, Confidence::High);
    }

    #[test]
    fn mid_title_parenthesized_volume_is_high_confidence() {
        let title = "身に覚えのない理由で婚約破棄されましたけれど、仮面の下が醜いだなんて、一体誰が言ったのかしら？【限定書きおろし小説付きコミックス版】（2） 身に覚えのない理由で婚約破棄されましたけれど、仮面の下が醜いだなんて、一体誰が言ったのかしら？【コミックス版】";
        let candidate = infer_candidate(title).unwrap();
        assert_eq!(
            candidate.display_name,
            "身に覚えのない理由で婚約破棄されましたけれど、仮面の下が醜いだなんて、一体誰が言ったのかしら?"
        );
        assert_eq!(candidate.volume_index, 2.0);
        assert_eq!(candidate.confidence, Confidence::High);
    }

    #[test]
    fn roman_numeral_volumes_are_contextual() {
        let candidate = infer_candidate("現実主義勇者の王国再建記IV").unwrap();
        assert_eq!(candidate.display_name, "現実主義勇者の王国再建記");
        assert_eq!(candidate.volume_index, 4.0);
        assert_eq!(candidate.confidence, Confidence::Contextual);

        let first = infer_candidate("現実主義勇者の王国再建記I").unwrap();
        assert_eq!(first.volume_index, 1.0);

        let inferred = infer_library(&[
            "現実主義勇者の王国再建記I".to_owned(),
            "現実主義勇者の王国再建記IV".to_owned(),
        ]);
        assert_eq!(
            inferred[0].as_ref().unwrap().name,
            "現実主義勇者の王国再建記"
        );
        assert_eq!(inferred[1].as_ref().unwrap().volume_index, 4.0);

        assert_eq!(infer_candidate("プロジェクトX"), None);
    }

    #[test]
    fn bare_numbers_after_sentence_enders_and_caps_runs_are_contextual() {
        let exclaim = infer_candidate("華麗に離縁してみせますわ！１").unwrap();
        assert_eq!(exclaim.display_name, "華麗に離縁してみせますわ!");
        assert_eq!(exclaim.volume_index, 1.0);
        assert_eq!(exclaim.confidence, Confidence::Contextual);

        let comic = infer_candidate("ダンジョン暮らしの元勇者 THE COMIC5").unwrap();
        assert_eq!(comic.display_name, "ダンジョン暮らしの元勇者 THE COMIC");
        assert_eq!(comic.volume_index, 5.0);
    }

    #[test]
    fn small_volume_numbers_form_singleton_series() {
        let inferred = infer_library(&["ウィズレイン王国物語 　～竜が花嫁～ 1".to_owned()]);
        assert_eq!(inferred[0].as_ref().unwrap().volume_index, 1.0);
    }
}
