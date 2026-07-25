/// Normalize markdown for stable hashing: UTF-8, LF newlines, strip trailing NULs.
pub fn normalize_markdown(input: &str) -> String {
    let mut s = input.replace("\r\n", "\n").replace('\r', "\n");
    if s.contains('\0') {
        s = s.replace('\0', "");
    }
    s
}

pub fn content_hash(normalized_markdown: &str) -> String {
    let hash = blake3::hash(normalized_markdown.as_bytes());
    hash.to_hex().to_string()
}

/// Rough plain-text derivation for search/excerpt (not a full markdown parser).
pub fn derive_plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut l = line.to_string();
        for prefix in [
            "# ", "## ", "### ", "#### ", "##### ", "###### ", "> ", "- ", "* ",
        ] {
            if let Some(rest) = l.strip_prefix(prefix) {
                l = rest.to_string();
                break;
            }
        }
        // Strip simple links/images: ![alt](url) / [text](url)
        l = strip_md_links(&l);
        out.push_str(&l);
        out.push('\n');
    }
    out
}

fn strip_md_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            i += 2;
            let mut alt = String::new();
            while i < chars.len() && chars[i] != ']' {
                alt.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '(' {
                i += 1;
                while i < chars.len() && chars[i] != ')' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
            }
            out.push_str(&alt);
            continue;
        }
        if chars[i] == '[' {
            i += 1;
            let mut text = String::new();
            while i < chars.len() && chars[i] != ']' {
                text.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '(' {
                i += 1;
                while i < chars.len() && chars[i] != ')' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
            }
            out.push_str(&text);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn derive_excerpt(markdown: &str, max_chars: usize) -> String {
    let text = derive_plain_text(markdown);
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    collapsed.chars().take(max_chars).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_crlf() {
        assert_eq!(
            content_hash(&normalize_markdown("a\r\nb")),
            content_hash(&normalize_markdown("a\nb"))
        );
    }
}
