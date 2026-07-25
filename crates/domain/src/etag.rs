#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ETag {
    pub revision: u64,
    pub hash_prefix: String,
}

pub fn format_etag(revision: u64, content_hash: &str) -> String {
    let prefix: String = content_hash.chars().take(16).collect();
    format!("{revision}:{prefix}")
}

pub fn parse_etag(etag: &str) -> Option<ETag> {
    let (rev, prefix) = etag.split_once(':')?;
    let revision = rev.parse().ok()?;
    if prefix.is_empty() {
        return None;
    }
    Some(ETag {
        revision,
        hash_prefix: prefix.to_string(),
    })
}

pub fn etag_matches(etag: &str, revision: u64, content_hash: &str) -> bool {
    match parse_etag(etag) {
        Some(e) => {
            e.revision == revision
                && content_hash.starts_with(&e.hash_prefix)
                && !e.hash_prefix.is_empty()
        }
        None => false,
    }
}
