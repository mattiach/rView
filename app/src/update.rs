//! Update check (Cargo feature `update-check`): with `updatenotifications`, at start
//! and at most every 4 hours, read the latest GitHub release of rView; a higher `tag_name` than
//! this build opens the update notice. Nothing is sent but the request itself.

use serde::Deserialize;
use std::time::Duration;

pub const RELEASES_API: &str = "https://api.github.com/repos/mattiach/rview/releases/latest";

/// Minimum time between two checks.
pub const INTERVAL: Duration = Duration::from_secs(4 * 3600);

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// `tag_name` without a leading "v".
    pub version: String,
    /// Body without its first line when that is "Changelog".
    pub notes: String,
    pub url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// Status of the update check.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Off,
    /// Not checked now: the last check (epoch s) is less than INTERVAL old.
    Recent(i64),
    Checking,
    UpToDate,
    Available(Release),
    Failed(String),
}

impl Status {
    pub fn text(&self) -> String {
        use crate::i18n::{t, tf};
        match self {
            Status::Off => t("update-status-off"),
            Status::Recent(at) => {
                let when = std::time::UNIX_EPOCH + Duration::from_secs((*at).max(0) as u64);
                tf(
                    "update-status-recent",
                    &[("when", &crate::platform::local_date_time(when))],
                )
            }
            Status::Checking => t("update-status-checking"),
            Status::UpToDate => t("update-status-uptodate"),
            Status::Available(r) => tf("update-status-available", &[("version", &r.version)]),
            Status::Failed(e) => tf("update-status-failed", &[("reason", e)]),
        }
    }
}

/// Numeric parts of a version ("v8.0.1" → [8, 0, 1]; "8.0-alpha" → [8, 0]).
fn parts(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map_while(|p| p.parse().ok())
        .collect()
}

/// `tag` is a higher version than `current` (missing parts count as 0: "8" = "8.0").
pub fn newer(tag: &str, current: &str) -> bool {
    let (mut a, mut b) = (parts(tag), parts(current));
    if a.is_empty() {
        return false;
    }
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// A check is due when the last one is older than INTERVAL (or in the future: clock changed).
pub fn due(last_check: i64, now: i64) -> bool {
    now < last_check || now - last_check >= INTERVAL.as_secs() as i64
}

pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Release notes for the notice: the body without a first "Changelog" line.
pub fn notes(body: &str) -> String {
    let body = body.trim_start();
    let mut lines = body.lines();
    match lines.next() {
        Some(first)
            if first
                .trim()
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("changelog") =>
        {
            lines.collect::<Vec<_>>().join("\n").trim().to_string()
        }
        _ => body.trim().to_string(),
    }
}

pub fn parse(json: &str) -> Result<Release, String> {
    let r: GithubRelease = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(Release {
        version: r.tag_name.trim().trim_start_matches(['v', 'V']).to_string(),
        notes: notes(r.body.as_deref().unwrap_or_default()),
        url: r.html_url.unwrap_or_else(|| crate::ui::notice::PROJECT_URL.to_string()),
    })
}

/// Latest release from `url` (the GitHub API, or a local server in tests).
pub fn fetch(url: &str) -> Result<Release, String> {
    let agent = crate::ui::openurl::agent();
    let resp = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let text = resp
        .into_body()
        .into_with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_interval_notes_json() {
        assert!(newer("8.1", "8.0.0"));
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("10.0", "9.9.9"), "numeric, not text");
        assert!(!newer("8.0", "8.0.0"), "missing parts = 0");
        assert!(!newer("0.1.0", "0.1.0") && !newer("0.0.9", "0.1.0"));
        assert!(!newer("nightly", "0.1.0"));
        assert!(newer("8.0.1-beta", "8.0.0"));

        let h = 3600;
        assert!(due(0, 1_000_000));
        assert!(!due(1_000_000, 1_000_000 + 3 * h));
        assert!(due(1_000_000, 1_000_000 + 4 * h));
        assert!(due(1_000_000, 999_000), "clock went back");

        assert_eq!(notes("Changelog\n- one\n- two"), "- one\n- two");
        assert_eq!(notes("## Changelog\r\n\r\n- one"), "- one");
        assert_eq!(notes("- just notes"), "- just notes");

        let r =
            parse(r#"{"tag_name":"v1.2","body":"Changelog\n- fix","html_url":"https://x/r/1.2","extra":1}"#).unwrap();
        assert_eq!(
            r,
            Release {
                version: "1.2".into(),
                notes: "- fix".into(),
                url: "https://x/r/1.2".into()
            }
        );
        let r = parse(r#"{"tag_name":"3.0","body":null}"#).unwrap();
        assert_eq!((r.notes.as_str(), r.url.as_str()), ("", crate::ui::notice::PROJECT_URL));
        assert!(parse(r#"{"message":"Not Found"}"#).is_err());
    }
}
