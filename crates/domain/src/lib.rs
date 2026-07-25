//! Pure domain logic: markdown normalize, hashing, etag, patch, errors.

mod error;
mod etag;
mod markdown;
mod patch;

pub use error::{ErrorCode, PandaError, PandaResult};
pub use etag::{etag_matches, format_etag, parse_etag, ETag};
pub use markdown::{content_hash, derive_excerpt, derive_plain_text, normalize_markdown};
pub use patch::{apply_markdown_patch, make_markdown_patch};

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
