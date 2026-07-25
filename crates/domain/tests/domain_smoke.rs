use domain::{
    apply_markdown_patch, content_hash, format_etag, make_markdown_patch, normalize_markdown,
    parse_etag,
};

#[test]
fn markdown_normalize_hash() {
    let a = normalize_markdown("hi\r\nthere\r\n");
    let b = normalize_markdown("hi\nthere\n");
    assert_eq!(a, b);
    assert_eq!(content_hash(&a), content_hash(&b));
}

#[test]
fn patch_roundtrip() {
    let old = "a\nb\nc\n";
    let new = "a\nb2\nc\n";
    let p = make_markdown_patch(old, new);
    assert_eq!(apply_markdown_patch(old, &p).unwrap(), new);
}

#[test]
fn etag_format() {
    let e = format_etag(9, "abcdef0123456789ffff");
    let p = parse_etag(&e).unwrap();
    assert_eq!(p.revision, 9);
    assert_eq!(p.hash_prefix, "abcdef0123456789");
}
