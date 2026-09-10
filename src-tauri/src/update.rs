//! Manual update check: **one** `GET` on the GitHub REST API, nothing else.
//!
//! The request is deliberately minimal (see `docs/design-updater.md`): no
//! authentication, no query string, no identifier of any kind. The only thing
//! that leaves the machine besides the URL is the application version, in the
//! `User-Agent` GitHub requires from every REST client.
//!
//! Everything except `http_get` is pure: the transport is injected, so the
//! whole state machine — version comparison, JSON parsing, error mapping — is
//! exercised by the tests without a socket.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::time::Duration;

/// The one endpoint this application ever contacts. `/releases/latest`
/// resolves to the latest **non-prerelease**, and answers 404 while the
/// repository is private.
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/CaseReed/wincleaner/releases/latest";

/// Long enough for a slow link, short enough that a hung connection does not
/// leave the button spinning forever.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the front end receives. `latest` is `None` when no public release
/// exists yet, which is the normal answer while the repository is private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: Option<String>,
    pub is_newer: bool,
    pub notes: Option<String>,
    pub url: Option<String>,
    pub published_at: Option<String>,
}

/// The four outcomes the user is told apart. `code` is what crosses the IPC
/// boundary: a stable token, so the wording lives in the front end and no
/// error message has to be matched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// DNS failure, no route, TLS failure, timeout.
    Offline,
    /// The endpoint answered, but there is no release to offer.
    NotAvailable(String),
    /// 60 requests/hour/IP, unauthenticated.
    RateLimited,
    /// The endpoint answered something we cannot read.
    Malformed(String),
}

impl UpdateError {
    pub fn code(&self) -> &'static str {
        match self {
            UpdateError::Offline => "offline",
            UpdateError::NotAvailable(_) => "not-available",
            UpdateError::RateLimited => "rate-limited",
            UpdateError::Malformed(_) => "malformed",
        }
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::Offline => write!(f, "offline"),
            UpdateError::NotAvailable(why) => write!(f, "not-available: {why}"),
            UpdateError::RateLimited => write!(f, "rate-limited"),
            UpdateError::Malformed(why) => write!(f, "malformed: {why}"),
        }
    }
}

/// The fields of a GitHub release this application reads. Everything else in
/// the payload — author, assets, reactions — is ignored on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    /// The canonical `semver::Version` form of `tag_name` (no leading `v`,
    /// no surrounding whitespace): what is shown and compared against.
    pub latest: String,
    pub body: Option<String>,
    pub html_url: Option<String>,
    pub published_at: Option<String>,
}

/// `html_url` is kept only when it points at this project's own GitHub pages:
/// anything else — a homoglyph domain, a `javascript:` URL, a plain
/// look-alike — is data the release body could have forged, and this is the
/// one field the front end both displays and offers to copy. A byte-wise
/// prefix match refuses homoglyphs: `wincIeaner` (capital I) is not
/// `wincleaner` one byte at a time, no Unicode confusable table needed.
const RELEASE_URL_PREFIX: &str = "https://github.com/CaseReed/wincleaner/";

/// Release notes are shown verbatim (as plain text — see `lib/updates.ts`),
/// so an unbounded body is still a nuisance, not an injection: this only
/// keeps one malicious or broken release from making the Settings screen
/// unusable.
const NOTES_MAX_CHARS: usize = 20_000;

fn truncate_chars(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        s.chars().take(max).collect()
    }
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: Option<String>,
    body: Option<String>,
    html_url: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// One HTTP answer, reduced to what the update logic distinguishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Drops a leading `v`: git tags carry it, semver does not.
fn strip_v(version: &str) -> &str {
    version.strip_prefix('v').unwrap_or(version)
}

/// Semver ordering of `current` against `latest`.
///
/// An unparseable version compares `Equal`, which is the safe answer: an
/// update is offered only on a strict `Less`, so garbage never produces one.
pub fn compare_versions(current: &str, latest: &str) -> Ordering {
    match (
        semver::Version::parse(strip_v(current.trim())),
        semver::Version::parse(strip_v(latest.trim())),
    ) {
        (Ok(current), Ok(latest)) => current.cmp(&latest),
        _ => Ordering::Equal,
    }
}

/// Reads the release payload. Drafts and prereleases are refused rather than
/// offered: `/releases/latest` should never return one, and if it ever does we
/// would rather show "nothing available" than push a beta at a stable user.
pub fn parse_release(json: &str) -> Result<ReleaseInfo, UpdateError> {
    let raw: RawRelease =
        serde_json::from_str(json).map_err(|e| UpdateError::Malformed(e.to_string()))?;
    if raw.draft || raw.prerelease {
        return Err(UpdateError::NotAvailable("No public release found".into()));
    }
    let tag_name = raw
        .tag_name
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| UpdateError::Malformed("release has no tag_name".into()))?;
    // A tag GitHub cannot be trusted to have kept as semver (a hand-pushed
    // tag, a typo) must not reach `compare_versions`, which reads anything
    // unparseable as "not newer" — silently hiding a real update is worse
    // than refusing the release outright.
    let version = semver::Version::parse(strip_v(tag_name.trim()))
        .map_err(|e| UpdateError::Malformed(format!("tag_name is not semver: {e}")))?;
    let html_url = raw
        .html_url
        .filter(|u| !u.trim().is_empty())
        .filter(|u| u.starts_with(RELEASE_URL_PREFIX));
    Ok(ReleaseInfo {
        tag_name,
        latest: version.to_string(),
        body: raw
            .body
            .filter(|b| !b.trim().is_empty())
            .map(|b| truncate_chars(b, NOTES_MAX_CHARS)),
        html_url,
        published_at: raw.published_at.filter(|p| !p.trim().is_empty()),
    })
}

/// Turns one HTTP answer into either the raw JSON body or the reason the user
/// is shown. `fetch` is injected so this mapping is tested without a network.
pub fn fetch_latest_release(
    fetch: impl FnOnce(&str) -> Result<HttpResponse, String>,
    url: &str,
) -> Result<String, UpdateError> {
    // A transport failure is indistinguishable from being offline from here,
    // and saying "check your connection" is the useful half of the truth.
    let response = fetch(url).map_err(|_| UpdateError::Offline)?;
    match response.status {
        200 => Ok(response.body),
        // Private repository, or no release published yet: GitHub answers 404
        // in both cases and we cannot tell them apart without a token.
        404 => Err(UpdateError::NotAvailable("No public release found".into())),
        403 | 429 if response.body.to_lowercase().contains("rate limit") => {
            Err(UpdateError::RateLimited)
        }
        // `max_redirects(0)` means a 3xx is handed back as an ordinary
        // response instead of being followed: the documented endpoint has no
        // legitimate reason to redirect, and following one would silently
        // break "one request, one host". Read as malformed rather than
        // offline: GitHub *did* answer, just not with something this
        // application trusts.
        300..=399 => Err(UpdateError::Malformed(format!(
            "unexpected redirect (status {})",
            response.status
        ))),
        _ => Err(UpdateError::Offline),
    }
}

/// The whole check, transport apart.
pub fn check_with(
    current: &str,
    fetch: impl FnOnce(&str) -> Result<HttpResponse, String>,
    url: &str,
) -> Result<UpdateCheck, UpdateError> {
    let body = fetch_latest_release(fetch, url)?;
    let release = parse_release(&body)?;
    Ok(UpdateCheck {
        is_newer: compare_versions(current, &release.latest) == Ordering::Less,
        current: current.to_string(),
        latest: Some(release.latest),
        notes: release.body,
        url: release.html_url,
        published_at: release.published_at,
    })
}

/// The largest response body this application will read. GitHub's release
/// JSON, notes included, is a few kilobytes; 256 KiB is generous headroom
/// against a compromised or misbehaving endpoint without ever letting a
/// single request hold an unbounded amount of memory.
const MAX_BODY_BYTES: u64 = 256 * 1024;

/// The configuration of the one agent this application ever builds. Split out
/// from `http_get` so the "one request, one host" invariant can be asserted
/// on the `Config` itself, without opening a socket.
///
/// - `max_redirects(0)`: the documented endpoint never redirects a plain GET;
///   following one would mean a second, unplanned request to an unplanned
///   host. `fetch_latest_release` reads any 3xx handed back as `Malformed`.
/// - `https_only(true)`: refuses to fall back to plaintext HTTP even if a
///   redirect or a misconfigured URL ever pointed at one.
/// - `proxy(None)`: ureq defaults to `Proxy::try_from_env()`, which would
///   route the request through whatever `HTTPS_PROXY`/`https_proxy` is set in
///   the environment — a second, user-invisible hop this application never
///   promised. Disabling it keeps the connection direct to `api.github.com`.
fn agent_config() -> ureq::config::Config {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("wincleaner/", env!("CARGO_PKG_VERSION")))
        // An HTTP error status is an answer, not a transport failure: we need
        // the status and the body back to tell 404 from a rate limit.
        .http_status_as_error(false)
        .max_redirects(0)
        .https_only(true)
        .proxy(None)
        .build()
}

/// The one place in the application that opens a socket.
///
/// Exactly two headers are set: the `User-Agent` GitHub requires from REST
/// clients, carrying nothing but the application version, and the `Accept`
/// that selects the documented media type. No authorization, no cookie, no
/// query string, no redirect, no proxy (see `agent_config`). The body is
/// capped at `MAX_BODY_BYTES`.
pub fn http_get(url: &str) -> Result<HttpResponse, String> {
    let agent = agent_config().new_agent();
    let mut response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok(HttpResponse { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, body: &str) -> impl FnOnce(&str) -> Result<HttpResponse, String> + '_ {
        move |_| {
            Ok(HttpResponse {
                status,
                body: body.to_string(),
            })
        }
    }

    const RELEASE: &str = r#"{
        "tag_name": "v0.3.0",
        "body": "Release notes\n\n### Added\n- A thing",
        "html_url": "https://github.com/CaseReed/wincleaner/releases/tag/v0.3.0",
        "published_at": "2026-09-10T12:00:00Z",
        "draft": false,
        "prerelease": false
    }"#;

    #[test]
    fn a_higher_release_is_newer() {
        assert_eq!(compare_versions("0.2.0", "0.3.0"), Ordering::Less);
        assert_eq!(compare_versions("0.3.0", "0.2.0"), Ordering::Greater);
    }

    #[test]
    fn the_tag_prefix_is_not_part_of_the_version() {
        assert_eq!(compare_versions("0.3.0", "v0.3.0"), Ordering::Equal);
        assert_eq!(compare_versions("v0.2.0", "v0.3.0"), Ordering::Less);
    }

    #[test]
    fn a_prerelease_sorts_below_its_release() {
        assert_eq!(compare_versions("0.3.0-beta.1", "0.3.0"), Ordering::Less);
        assert_eq!(compare_versions("0.3.0", "0.3.0-beta.1"), Ordering::Greater);
    }

    #[test]
    fn identical_versions_are_equal() {
        assert_eq!(compare_versions("0.2.0", "0.2.0"), Ordering::Equal);
    }

    /// Safety net rather than a nicety: an unreadable version must never be
    /// read as "an update is available".
    #[test]
    fn an_unparseable_version_never_offers_an_update() {
        assert_eq!(compare_versions("0.2.0", "latest"), Ordering::Equal);
        assert_eq!(compare_versions("nightly", "0.3.0"), Ordering::Equal);
    }

    #[test]
    fn a_release_yields_its_tag_notes_url_and_date() {
        let release = parse_release(RELEASE).unwrap();
        assert_eq!(release.tag_name, "v0.3.0");
        assert_eq!(release.latest, "0.3.0");
        assert_eq!(
            release.body.as_deref(),
            Some("Release notes\n\n### Added\n- A thing")
        );
        assert_eq!(
            release.html_url.as_deref(),
            Some("https://github.com/CaseReed/wincleaner/releases/tag/v0.3.0")
        );
        assert_eq!(release.published_at.as_deref(), Some("2026-09-10T12:00:00Z"));
    }

    #[test]
    fn a_draft_is_not_a_release() {
        let json = RELEASE.replace("\"draft\": false", "\"draft\": true");
        assert_eq!(
            parse_release(&json),
            Err(UpdateError::NotAvailable("No public release found".into()))
        );
    }

    #[test]
    fn a_prerelease_is_not_offered() {
        let json = RELEASE.replace("\"prerelease\": false", "\"prerelease\": true");
        assert_eq!(
            parse_release(&json),
            Err(UpdateError::NotAvailable("No public release found".into()))
        );
    }

    #[test]
    fn a_release_without_a_tag_is_malformed() {
        let json = r#"{"body": "notes", "draft": false, "prerelease": false}"#;
        assert!(matches!(
            parse_release(json),
            Err(UpdateError::Malformed(_))
        ));
    }

    #[test]
    fn a_body_that_is_not_json_is_malformed() {
        assert!(matches!(
            parse_release("<html>404</html>"),
            Err(UpdateError::Malformed(_))
        ));
    }

    /// The absent fields are optional, not an error: a release published with
    /// an empty body is still a release.
    #[test]
    fn empty_optional_fields_read_as_absent() {
        let json = r#"{"tag_name": "v0.3.0", "body": "", "html_url": "", "published_at": ""}"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.body, None);
        assert_eq!(release.html_url, None);
        assert_eq!(release.published_at, None);
    }

    /// A homoglyph host (capital `I` standing in for a lowercase `l`) is not
    /// this project's GitHub page byte-for-byte, and a byte-wise prefix
    /// match is exactly what refuses it — no confusable table needed.
    #[test]
    fn a_homoglyph_url_is_dropped() {
        let json = RELEASE.replace(
            "https://github.com/CaseReed/wincleaner/",
            "https://github.com/CaseReed/wincIeaner/",
        );
        let release = parse_release(&json).unwrap();
        assert_eq!(release.html_url, None);
    }

    #[test]
    fn a_javascript_url_is_dropped() {
        let json = RELEASE.replace(
            "https://github.com/CaseReed/wincleaner/releases/tag/v0.3.0",
            "javascript:alert(1)",
        );
        let release = parse_release(&json).unwrap();
        assert_eq!(release.html_url, None);
    }

    #[test]
    fn a_url_under_the_project_page_is_kept() {
        let release = parse_release(RELEASE).unwrap();
        assert_eq!(
            release.html_url.as_deref(),
            Some("https://github.com/CaseReed/wincleaner/releases/tag/v0.3.0")
        );
    }

    #[test]
    fn a_non_semver_tag_is_malformed() {
        let json = RELEASE.replace("\"v0.3.0\"", "\"latest-nightly\"");
        assert!(matches!(
            parse_release(&json),
            Err(UpdateError::Malformed(_))
        ));
    }

    #[test]
    fn notes_are_truncated_to_twenty_thousand_characters() {
        let json = format!(
            r#"{{"tag_name": "v0.3.0", "body": "{}", "draft": false, "prerelease": false}}"#,
            "a".repeat(30_000)
        );
        let release = parse_release(&json).unwrap();
        assert_eq!(release.body.unwrap().chars().count(), NOTES_MAX_CHARS);
    }

    #[test]
    fn a_transport_failure_reads_as_offline() {
        let fetch = |_: &str| Err("dns error".to_string());
        assert_eq!(
            fetch_latest_release(fetch, LATEST_RELEASE_URL),
            Err(UpdateError::Offline)
        );
    }

    /// The state of the repository today: private, so unauthenticated GETs get
    /// a 404 and the user is told there is nothing to install, not that
    /// something broke.
    #[test]
    fn a_404_reads_as_no_public_release() {
        assert_eq!(
            fetch_latest_release(answer(404, "{\"message\":\"Not Found\"}"), LATEST_RELEASE_URL),
            Err(UpdateError::NotAvailable("No public release found".into()))
        );
    }

    #[test]
    fn a_403_mentioning_the_rate_limit_reads_as_rate_limited() {
        let body = r#"{"message":"API rate limit exceeded for 1.2.3.4."}"#;
        assert_eq!(
            fetch_latest_release(answer(403, body), LATEST_RELEASE_URL),
            Err(UpdateError::RateLimited)
        );
    }

    /// A 403 that is not about the quota is not something we can explain, and
    /// "could not reach GitHub" is closer to the truth than a wrong reason.
    #[test]
    fn a_403_that_is_not_about_the_quota_reads_as_offline() {
        assert_eq!(
            fetch_latest_release(answer(403, "{\"message\":\"Forbidden\"}"), LATEST_RELEASE_URL),
            Err(UpdateError::Offline)
        );
    }

    /// `max_redirects(0)` hands a 3xx back as an ordinary response instead of
    /// an error (see `agent_config`); this is the mapping that turns it into
    /// a user-facing outcome instead of being read as a 2xx body.
    #[test]
    fn a_redirect_reads_as_malformed() {
        assert!(matches!(
            fetch_latest_release(answer(302, ""), LATEST_RELEASE_URL),
            Err(UpdateError::Malformed(_))
        ));
    }

    #[test]
    fn a_server_error_reads_as_offline() {
        assert_eq!(
            fetch_latest_release(answer(500, "boom"), LATEST_RELEASE_URL),
            Err(UpdateError::Offline)
        );
    }

    #[test]
    fn a_200_hands_the_body_back_untouched() {
        assert_eq!(
            fetch_latest_release(answer(200, RELEASE), LATEST_RELEASE_URL),
            Ok(RELEASE.to_string())
        );
    }

    #[test]
    fn the_url_is_handed_to_the_transport_unchanged() {
        let seen = std::cell::Cell::new(String::new());
        let _ = fetch_latest_release(
            |url| {
                seen.set(url.to_string());
                Ok(HttpResponse {
                    status: 200,
                    body: RELEASE.to_string(),
                })
            },
            LATEST_RELEASE_URL,
        );
        assert_eq!(seen.take(), LATEST_RELEASE_URL);
        // No query string: nothing about this machine is appended to the URL.
        assert!(!LATEST_RELEASE_URL.contains('?'));
    }

    #[test]
    fn a_newer_release_is_reported_with_its_notes() {
        let check = check_with("0.2.0", answer(200, RELEASE), LATEST_RELEASE_URL).unwrap();
        assert_eq!(check.current, "0.2.0");
        assert_eq!(check.latest.as_deref(), Some("0.3.0"));
        assert!(check.is_newer);
        assert_eq!(
            check.notes.as_deref(),
            Some("Release notes\n\n### Added\n- A thing")
        );
        assert_eq!(check.published_at.as_deref(), Some("2026-09-10T12:00:00Z"));
    }

    #[test]
    fn the_same_version_is_not_newer() {
        let check = check_with("0.3.0", answer(200, RELEASE), LATEST_RELEASE_URL).unwrap();
        assert_eq!(check.latest.as_deref(), Some("0.3.0"));
        assert!(!check.is_newer);
    }

    #[test]
    fn an_older_release_is_never_offered() {
        let check = check_with("0.4.0", answer(200, RELEASE), LATEST_RELEASE_URL).unwrap();
        assert!(!check.is_newer);
    }

    #[test]
    fn each_failure_carries_a_stable_code_for_the_front_end() {
        assert_eq!(UpdateError::Offline.code(), "offline");
        assert_eq!(
            UpdateError::NotAvailable("x".into()).code(),
            "not-available"
        );
        assert_eq!(UpdateError::RateLimited.code(), "rate-limited");
        assert_eq!(UpdateError::Malformed("x".into()).code(), "malformed");
    }

    #[test]
    fn the_endpoint_is_the_documented_one_and_carries_no_credential() {
        assert_eq!(
            LATEST_RELEASE_URL,
            "https://api.github.com/repos/CaseReed/wincleaner/releases/latest"
        );
        assert!(LATEST_RELEASE_URL.starts_with("https://"));
        assert!(!LATEST_RELEASE_URL.contains('@'));
    }

    /// Locks down the three settings that keep this to one request, one
    /// host, without opening a socket: built straight from `agent_config`,
    /// not re-derived by hand.
    #[test]
    fn the_agent_follows_no_redirect_uses_no_proxy_and_is_https_only() {
        let config = agent_config();
        assert_eq!(config.max_redirects(), 0);
        assert!(config.https_only());
        assert!(config.proxy().is_none());
    }
}
