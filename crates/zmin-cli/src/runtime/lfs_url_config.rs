//! Git LFS URLConfig matching shared by credential and HTTP policy lookup.

use std::fmt;

use zmin_http_transport::HttpUrl;

use super::decode_lfs_url_path;

const MAX_LFS_URL_SCOPE_BYTES: usize = 16 * 1024;

/// Parsed URL matching material. Hosts are ASCII case-folded while usernames
/// and decoded slash-delimited path elements remain byte exact.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct LfsUrlScope {
    scheme: String,
    username: Option<Vec<u8>>,
    host: String,
    port: u16,
    path: Vec<Vec<u8>>,
}

impl fmt::Debug for LfsUrlScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsUrlScope")
            .field("has_username", &self.username.is_some())
            .field("host_labels", &self.host.split('.').count())
            .field("path_elements", &self.path.len())
            .finish_non_exhaustive()
    }
}

impl LfsUrlScope {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        if raw.is_empty() || raw.len() > MAX_LFS_URL_SCOPE_BYTES {
            return None;
        }
        let scheme_end = raw.find("://")?;
        let scheme = raw[..scheme_end].to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https") {
            return None;
        }
        let remainder = &raw[scheme_end + 3..];
        let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
        let mut authority = &remainder[..authority_end];
        if authority.is_empty() {
            return None;
        }
        let username = if let Some((userinfo, host)) = authority.rsplit_once('@') {
            authority = host;
            let (username, password) = userinfo
                .split_once(':')
                .map_or((userinfo, None), |(username, password)| {
                    (username, Some(password))
                });
            let username = decode_lfs_url_path(username).ok()?;
            if password.is_some_and(|password| decode_lfs_url_path(password).is_err()) {
                return None;
            }
            Some(username)
        } else {
            None
        };
        let (host, port) = parse_authority(authority, &scheme)?;
        let path_and_query = &remainder[authority_end..];
        let raw_path = path_and_query
            .split_once(['?', '#'])
            .map_or(path_and_query, |(path, _)| path);
        let path = decode_lfs_url_path(raw_path).ok()?;
        Some(Self {
            scheme,
            username,
            host,
            port,
            path: path
                .split(|byte| *byte == b'/')
                .filter(|part| !part.is_empty())
                .map(<[u8]>::to_vec)
                .collect(),
        })
    }

    pub(crate) fn from_transport(url: &HttpUrl) -> Option<Self> {
        Self::parse(url.as_str())
    }

    pub(crate) fn match_score(&self, target: &Self) -> Option<(usize, usize, usize)> {
        if self.scheme != target.scheme || self.port != target.port {
            return None;
        }
        let host_score = compare_hosts(&target.host, &self.host)?;
        let path_score = compare_paths(&target.path, &self.path)?;
        let user_score = match self.username.as_deref() {
            Some(username) if target.username.as_deref() == Some(username) => 1,
            Some(_) => return None,
            None => 0,
        };
        Some((host_score, path_score, user_score))
    }
}

fn parse_authority(authority: &str, scheme: &str) -> Option<(String, u16)> {
    if authority.starts_with('[') {
        let closing = authority.find(']')?;
        let host = authority[1..closing].to_ascii_lowercase();
        let suffix = &authority[closing + 1..];
        let port = if suffix.is_empty() {
            default_port(scheme)
        } else {
            suffix.strip_prefix(':')?.parse::<u16>().ok()?
        };
        valid_host(&host).then_some((host, port))
    } else {
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => {
                if host.contains(':')
                    || port.is_empty()
                    || !port.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return None;
                }
                (host, Some(port))
            }
            None => (authority, None),
        };
        let host = host.to_ascii_lowercase();
        let port = port
            .map(str::parse::<u16>)
            .transpose()
            .ok()?
            .unwrap_or_else(|| default_port(scheme));
        (valid_host(&host) && !host.contains('%')).then_some((host, port))
    }
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && !host.contains(['\\', '@', '[', ']'])
        && !host
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "http" { 80 } else { 443 }
}

fn compare_hosts(target: &str, scope: &str) -> Option<usize> {
    let target_parts = target.split('.').collect::<Vec<_>>();
    let scope_parts = scope.split('.').collect::<Vec<_>>();
    if target_parts.len() != scope_parts.len() {
        return None;
    }
    let mut score = target_parts.len() + 1;
    for (target_part, scope_part) in target_parts.iter().zip(scope_parts) {
        if scope_part == "*" {
            score = score.saturating_sub(1);
        } else if target_part != &scope_part {
            return None;
        }
    }
    Some(score)
}

fn compare_paths(target: &[Vec<u8>], scope: &[Vec<u8>]) -> Option<usize> {
    if target.len() < scope.len() {
        return None;
    }
    let mut score = 1;
    for (index, scope_part) in scope.iter().enumerate() {
        let target_part = &target[index];
        if target_part == scope_part {
            score += 2;
        } else if target_part.ends_with(b".git")
            && target_part.len() > 4
            && &target_part[..target_part.len() - 4] == scope_part.as_slice()
            && target.get(index + 1).map(Vec::as_slice) == Some(b"info")
            && target.get(index + 2).map(Vec::as_slice) == Some(b"lfs")
        {
            score += 1;
        } else {
            return None;
        }
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_uses_scheme_port_whole_labels_path_username_and_default_lfs_mapping() {
        let target =
            LfsUrlScope::parse("https://alice@api.example.com:443/repo.git/info/lfs/objects/batch")
                .expect("target");
        let exact = LfsUrlScope::parse("https://alice@api.example.com/repo").expect("exact");
        let wildcard = LfsUrlScope::parse("https://*.example.com/repo").expect("wildcard");
        assert!(exact.match_score(&target) > wildcard.match_score(&target));
        assert!(
            LfsUrlScope::parse("http://alice@api.example.com/repo")
                .expect("scheme")
                .match_score(&target)
                .is_none()
        );
        assert!(
            LfsUrlScope::parse("https://bob@api.example.com/repo")
                .expect("user")
                .match_score(&target)
                .is_none()
        );
        assert!(
            LfsUrlScope::parse("https://*.other.example.com/repo")
                .expect("labels")
                .match_score(&target)
                .is_none()
        );
    }

    #[test]
    fn matcher_keeps_decoded_path_bytes_and_slash_boundaries_exact() {
        let target =
            LfsUrlScope::parse("https://example.com/a%20b/repo.git/info/lfs").expect("target");
        assert!(
            LfsUrlScope::parse("https://example.com/a%20b/repo")
                .expect("matching")
                .match_score(&target)
                .is_some()
        );
        assert!(
            LfsUrlScope::parse("https://example.com/a/re")
                .expect("prefix")
                .match_score(&target)
                .is_none()
        );
    }

    #[test]
    fn matcher_decodes_usernames_ignores_passwords_and_rejects_invalid_authorities() {
        let target = LfsUrlScope::parse("https://a%20b@example.com/repo").expect("target");
        let configured =
            LfsUrlScope::parse("https://a%20b:ignored@example.com/repo").expect("configured");
        assert!(configured.match_score(&target).is_some());
        for invalid in [
            "https://example.com:not-a-port/repo",
            "https://example.com:99999/repo",
            "https://bad\\host/repo",
            "https://bad host/repo",
            "https://bad%zz.example/repo",
            "https://bad%zz@example.com/repo",
        ] {
            assert!(LfsUrlScope::parse(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn debug_reports_only_shape_and_scope_input_is_bounded() {
        let scope = LfsUrlScope::parse(
            "https://private-user:private-password@secret.example/private/repository",
        )
        .expect("scope");
        let debug = format!("{scope:?}");
        for secret in [
            "private-user",
            "private-password",
            "secret.example",
            "private",
            "repository",
        ] {
            assert!(!debug.contains(secret), "{secret}");
        }
        assert!(debug.contains("has_username"));
        assert!(
            LfsUrlScope::parse(&format!(
                "https://example.com/{}",
                "x".repeat(MAX_LFS_URL_SCOPE_BYTES)
            ))
            .is_none()
        );
    }
}
