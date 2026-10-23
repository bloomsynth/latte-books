//! Translations: `main/getLanguageMap.ts`, `fyo/utils/translation.ts`,
//! `utils/translationHelpers.ts` and `src/utils/language.ts` @ v0.36.0.
//!
//! Differences from Books:
//! - The translation files are the ones bundled with Latte (`vendor/books/translations`).
//!   Books also downloads newer ones from GitHub.
//! - Arabic and Persian are left out: Latte has no right-to-left layout yet.

use std::collections::HashMap;

/// `DEFAULT_LANGUAGE`.
pub const DEFAULT_LANGUAGE: &str = "English";

/// `languageCodeMap` without Arabic and Persian: each language and its file.
pub const LANGUAGES: [(&str, &str); 17] = [
    ("Catalan", "ca-ES"),
    ("Danish", "da"),
    ("Dutch", "nl"),
    ("English", "en"),
    ("French", "fr"),
    ("German", "de"),
    ("Gujarati", "gu"),
    ("Hindi", "hi"),
    ("Indonesian", "id"),
    ("Korean", "ko"),
    ("Nepali", "np"),
    ("Portuguese", "pt"),
    ("Simplified Chinese", "zh-CN"),
    ("Traditional Chinese", "zh-Hant"),
    ("Spanish", "es"),
    ("Swedish", "sv"),
    ("Turkish", "tr"),
];

macro_rules! files {
    ($($code:literal),*) => {
        &[$(($code, include_str!(concat!("../../../vendor/books/translations/", $code, ".csv")))),*]
    };
}

const FILES: &[(&str, &str)] = files!(
    "ca-ES", "da", "nl", "fr", "de", "gu", "hi", "id", "ko", "np", "pt", "zh-CN", "zh-Hant", "es",
    "sv", "tr"
);

/// `LanguageMap`: each source string and its translation.
#[derive(Debug, Clone, Default)]
pub struct LanguageMap {
    map: HashMap<String, String>,
}

impl LanguageMap {
    /// The map for a language by its name ("German"), or `None` for English and
    /// languages Latte doesn't have.
    pub fn for_language(language: &str) -> Option<Self> {
        let code = LANGUAGES.iter().find(|(name, _)| *name == language)?.1;
        let (_, csv) = FILES.iter().find(|(c, _)| *c == code)?;
        Some(Self::from_csv(csv))
    }

    /// `getMapFromCsv`.
    pub fn from_csv(csv: &str) -> Self {
        let map = parse_csv(csv)
            .into_iter()
            .filter(|row| row.len() >= 2 && !row[0].is_empty() && !row[1].is_empty())
            .map(|mut row| {
                let translation = row.swap_remove(1);
                (row.swap_remove(0), translation)
            })
            .collect();
        Self { map }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The translation of a string without arguments, or `None`.
    pub fn get(&self, source: &str) -> Option<&str> {
        self.map
            .get(&whitespace_sanitized(source))
            .map(String::as_str)
    }

    /// `t` for a string with `${0}`-style arguments: the translated format, with the
    /// arguments put back in the order the translation names them.
    pub fn translate(&self, format: &str, args: &[&str]) -> String {
        let format = whitespace_sanitized(format);
        let translated = self.map.get(&format).cloned().unwrap_or(format);
        stitch(&translated, args)
    }
}

/// `t` without a language map: the format with its arguments.
pub fn stitch(format: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(start) = rest.find("${") {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        let index = &rest[start + 2..start + len];
        if let Some(arg) = index.parse::<usize>().ok().and_then(|i| args.get(i)) {
            out.push_str(arg);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    whitespace_sanitized(&out)
}

/// `getWhitespaceSanitized`.
fn whitespace_sanitized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// RFC 4180 CSV (`parseCSV`).
///
/// Difference from Books: a quote opens a quoted field only at the start of a field and
/// is kept as text anywhere else. Books' `splitCsvBlock` toggles on every quote (it reads
/// `c[i + 1]` for `text[i + 1]`), so one stray quote, as in the German file, joins the
/// rest of the file into a few rows and drops most of its translations.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let starts = at_start;
        at_start = false;
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' if in_quotes => in_quotes = false,
            '"' if starts => in_quotes = true,
            ',' if !in_quotes => {
                row.push(std::mem::take(&mut cell));
                at_start = true;
            }
            '\r' if !in_quotes => at_start = starts,
            '\n' if !in_quotes => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_translation_file() {
        let map = LanguageMap::for_language("German").unwrap();
        assert_eq!(map.len(), 979);
        assert_eq!(
            map.translate("${0} ${1} already exists.", &["Item", "Pen"]),
            "Item Pen existiert bereits"
        );
        // The German file drops the second argument.
        assert_eq!(
            map.translate("${0} ${1} does not exist", &["Item", "Pen"]),
            "Item existiert nicht"
        );
        assert!(LanguageMap::for_language("English").is_none());
        assert!(LanguageMap::for_language("Arabic").is_none());
    }

    #[test]
    fn every_bundled_language_loads() {
        for (language, code) in LANGUAGES {
            if code != "en" {
                assert!(LanguageMap::for_language(language).is_some_and(|m| !m.is_empty()));
            }
        }
    }

    #[test]
    fn stitches_arguments() {
        assert_eq!(stitch("Hello  ${0}, ${1}!", &["a", "b"]), "Hello a, b!");
        let csv = "\"a, b\",\"x \"\"y\"\"\",\n\"multi\nline\",\"z\",\n";
        let rows = parse_csv(csv);
        assert_eq!(rows[0], vec!["a, b", "x \"y\"", ""]);
        assert_eq!(rows[1][0], "multi\nline");
    }
}
