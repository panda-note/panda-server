use crate::error::{PandaError, PandaResult};
use diffy::{apply, create_patch, PatchFormatter};

/// Unified diff patch (diffy format) of old -> new.
pub fn make_markdown_patch(old: &str, new: &str) -> String {
    let patch = create_patch(old, new);
    let formatted = PatchFormatter::new().fmt_patch(&patch).to_string();
    formatted
}

pub fn apply_markdown_patch(base: &str, patch_text: &str) -> PandaResult<String> {
    let patch = diffy::Patch::from_str(patch_text)
        .map_err(|e| PandaError::invalid(format!("invalid markdown_patch: {e}")))?;
    apply(base, &patch).map_err(|e| PandaError::invalid(format!("patch apply failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_roundtrip() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline2 changed\nline3\n";
        let p = make_markdown_patch(old, new);
        let out = apply_markdown_patch(old, &p).unwrap();
        assert_eq!(out, new);
    }
}
