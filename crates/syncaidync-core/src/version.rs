//! Parse and compare dotted version numbers from git tags.

/// Parse `v1.2.3` or `1.2.3` into `(major, minor, patch)`. Extra pre-release suffixes fail.
pub fn parse(input: &str) -> Option<(u64, u64, u64)> {
    let s = input.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    // Reject pre-release markers glued to the patch (e.g. 1.0.0-rc.1 already fails parse).
    Some((major, minor, patch))
}

/// True when `latest` is strictly greater than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_and_without_v() {
        assert_eq!(parse("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse("junk"), None);
        assert_eq!(parse("1.2"), None);
        assert_eq!(parse("v1.2.3-rc1"), None);
    }

    #[test]
    fn newer_compare() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("nope", "0.1.0"));
    }
}
