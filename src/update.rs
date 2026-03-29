use serde::Deserialize;

pub const RELEASES_PAGE_URL: &str = "https://github.com/edoli/edolview-rs/releases";
const LATEST_RELEASE_API_URL: &str = "https://api.github.com/repos/edoli/edolview-rs/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub html_url: String,
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    html_url: String,
}

pub fn check_for_update() -> Result<Option<AvailableUpdate>, String> {
    let release = ureq::get(LATEST_RELEASE_API_URL)
        .header("User-Agent", "edolview-rs")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Failed to query GitHub releases: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read GitHub release response: {e}"))
        .and_then(|body| {
            serde_json::from_str::<ReleaseResponse>(&body)
                .map_err(|e| format!("Failed to parse GitHub release response: {e}"))
        })?;

    if is_newer_version(release.tag_name.as_str(), CURRENT_VERSION) {
        Ok(Some(AvailableUpdate {
            version: normalize_version_label(release.tag_name.as_str()),
            html_url: release.html_url,
        }))
    } else {
        Ok(None)
    }
}

fn normalize_version_label(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with(['v', 'V']) {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate_parts), Some(current_parts)) => compare_versions(&candidate_parts, &current_parts).is_gt(),
        _ => normalize_version_label(candidate) != normalize_version_label(current),
    }
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;

    let mut parts = Vec::new();
    for part in core.split('.') {
        if part.is_empty() {
            return None;
        }
        parts.push(part.parse::<u64>().ok()?);
    }
    Some(parts)
}

fn compare_versions(left: &[u64], right: &[u64]) -> std::cmp::Ordering {
    let max_len = left.len().max(right.len());
    for idx in 0..max_len {
        let lhs = *left.get(idx).unwrap_or(&0);
        let rhs = *right.get(idx).unwrap_or(&0);
        match lhs.cmp(&rhs) {
            std::cmp::Ordering::Equal => {}
            non_eq => return non_eq,
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::is_newer_version;

    #[test]
    fn detects_newer_semver_versions() {
        assert!(is_newer_version("v0.4.20", "0.4.19"));
        assert!(is_newer_version("0.5.0", "0.4.20"));
        assert!(is_newer_version("0.4.20.1", "0.4.20"));
    }

    #[test]
    fn ignores_same_or_older_versions() {
        assert!(!is_newer_version("v0.4.20", "0.4.20"));
        assert!(!is_newer_version("0.4.19", "0.4.20"));
        assert!(!is_newer_version("0.4", "0.4.0"));
    }
}
