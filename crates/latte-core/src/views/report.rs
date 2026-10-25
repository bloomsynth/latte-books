//! Reporting an issue: `getIssueUrlQuery`, `reportIssue` and the `truncate` of
//! `handleErrorWithDialog` in `src/errorHandling.ts` @ v0.36.0.
//!
//! Differences from Books: the issue is opened on Latte Books' repository, and the body
//! has no feature flags; Books lists the boolean settings in it.

/// Where issues are opened.
pub const ISSUES_URL: &str = "https://github.com/bloomsynth/latte-books/issues/new?labels=bug";

/// What an issue says about the app, under Info.
#[derive(Debug, Clone, Default)]
pub struct ReportInfo {
    pub version: String,
    pub platform: String,
    pub language: String,
}

/// `getIssueUrlQuery`: a new issue with Books' description, steps and info, and the error
/// when there is one.
pub fn issue_url(error: Option<&str>, info: &ReportInfo) -> String {
    let mut body = vec![
        "<h2>Description</h2>".to_owned(),
        "Add some description...".to_owned(),
        String::new(),
        "<h2>Steps to Reproduce</h2>".to_owned(),
        "Add steps to reproduce the error...".to_owned(),
        String::new(),
        "<h2>Info</h2>".to_owned(),
        String::new(),
    ];
    if let Some(error) = error {
        body.push(format!("**Error**: _{error}_"));
        body.push(String::new());
    }
    body.push(format!("**Version**: `{}`", info.version));
    body.push(format!("**Platform**: `{}`", info.platform));
    let language = if info.language.is_empty() {
        "-"
    } else {
        &info.language
    };
    body.push(format!("**Language**: `{language}`"));
    format!(
        "{ISSUES_URL}&body={}",
        encode_uri_component(&body.join("\n"))
    )
}

/// JavaScript's `encodeURIComponent`.
pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// lodash's `truncate` with `length`: text longer than that is cut to fit with "...".
pub fn truncate(text: &str, length: usize) -> String {
    if text.chars().count() <= length {
        return text.to_owned();
    }
    let kept: String = text.chars().take(length.saturating_sub(3)).collect();
    format!("{kept}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_as_javascript_does() {
        assert_eq!(
            encode_uri_component("a b&c/é!*'()~"),
            "a%20b%26c%2F%C3%A9!*'()~"
        );
        assert_eq!(encode_uri_component("**x**\n`y`"), "**x**%0A%60y%60");
    }

    #[test]
    fn truncates_as_lodash_does() {
        assert_eq!(truncate("short", 128), "short");
        let long = "x".repeat(130);
        let cut = truncate(&long, 128);
        assert_eq!(cut.chars().count(), 128);
        assert!(cut.ends_with("xx..."));
    }

    #[test]
    fn the_issue_carries_the_error_and_info() {
        let info = ReportInfo {
            version: "0.1.0".into(),
            platform: "Linux".into(),
            language: String::new(),
        };
        let url = issue_url(Some("Error: file is locked"), &info);
        assert!(url.starts_with(ISSUES_URL));
        assert!(url.contains("**Error**%3A%20_Error"));
        assert!(url.contains("_Error%3A%20file%20is%20locked_"));
        assert!(url.contains("**Version**%3A%20%600.1.0%60"));
        assert!(url.contains("**Language**%3A%20%60-%60"));
        let url = issue_url(None, &info);
        assert!(!url.contains("Error**"));
    }
}
