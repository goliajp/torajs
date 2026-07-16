//! Type-annotation shape helpers split from `parser.rs` (rotation
//! 119 chunk 7, file-size decomp): the two shipped-since-chunk-437
//! free fns take ~45 LOC combined and have zero coupling to the
//! `Parser` state — a clean sibling extraction.

/// V3-18 wedge — recognise a syntactic IdentifierName per JS spec
/// §11.6.2: ASCII letters / `_` / `$` for the first byte; same set
/// plus digits for the rest. Used to fold `obj["x"]` into
/// `obj.x` at parse time when the bracket-index is a string
/// literal whose content is a legal identifier; non-ident strings
/// (`obj["a-b"]`, `obj["1"]`, `obj[""]`) stay as Index so the
/// existing Array / String paths handle them.
pub(super) fn is_identifier_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return false;
    }
    for &b in &bytes[1..] {
        if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'$') {
            return false;
        }
    }
    true
}

/// V3-18 wedge — strip the standard generator/iterator wrapper from
/// a return-type annotation per TS spec §3.6.4. Recognized shapes
/// (all single-arg, all collapsing to the inner yield type T):
///   Generator<T>          (also Generator<T, R, N> — extras ignored)
///   IterableIterator<T>
///   Iterator<T>
///   Iterable<T>
/// The parser's flat-ann encoder writes these as `Head<T>` (or
/// `Head<T|R|N>`), so a depth-aware scan for the first `<` and the
/// trailing `>` is enough.
pub(super) fn unwrap_generator_return_ann(ann: &str) -> String {
    let Some(open) = ann.find('<') else {
        return ann.to_string();
    };
    if !ann.ends_with('>') {
        return ann.to_string();
    }
    let head = &ann[..open];
    if !matches!(
        head,
        "Generator" | "IterableIterator" | "Iterator" | "Iterable" | "AsyncGenerator"
    ) {
        return ann.to_string();
    }
    let inner = &ann[open + 1..ann.len() - 1];
    // Take only the first type-arg (yield type). TS Generator has
    // additional Return/Next type args; the subset runtime collapses
    // them so dropping is the only sensible thing.
    let mut depth: i32 = 0;
    for (i, b) in inner.bytes().enumerate() {
        match b {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth -= 1,
            b'|' if depth == 0 => return inner[..i].to_string(),
            _ => {}
        }
    }
    inner.to_string()
}
