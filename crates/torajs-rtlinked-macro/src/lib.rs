//! `runtime_linked_image!(BASE, "ENV_A", "ENV_B", ...)` — `BASE` is the
//! image's preferred load address, each string names an env var the
//! embedding crate's build script set to a staticlib path, in link
//! search order. The macro builds the runtime image from those
//! archives (stripped as `runtime_image!` strips it, so both images
//! hold the same runtime), links it into one fixed-layout image
//! ([`torajs_link::rtlinked_format`]) and expands to a `&'static [u8]`
//! whose first byte sits on a [`SEGMENT_ALIGN`] boundary. The image's
//! templates are segment-aligned within the blob, so they are
//! segment-aligned in the embedding binary's memory and file too.
//!
//! `runtime_image!("ENV_A", "ENV_B", ...)` expands to the runtime
//! image `tr build` links against: the same archives, every atom no
//! program can reach already stripped
//! ([`torajs_link::runtime_prestrip`]), as a byte-string literal.
//!
//! Every archive is also named in an unused `include_bytes!`, so the
//! embedding crate rebuilds when one changes.

use proc_macro::{Delimiter, Literal, TokenStream, TokenTree};
use torajs_link::rtlinked_build::build_rtlinked_image;
use torajs_link::rtlinked_format::SEGMENT_ALIGN;
use torajs_link::runtime_prestrip::prestripped_runtime_image;

fn parse_base(lit: &str) -> u64 {
    let digits: String = lit.chars().filter(|c| *c != '_').collect();
    let parsed = match digits.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => digits.parse(),
    };
    parsed.unwrap_or_else(|e| panic!("runtime_linked_image!: base {lit}: {e}"))
}

fn env_path(lit: &str) -> String {
    let var = lit
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| panic!("expected a string literal, got {lit}"));
    std::env::var(var).unwrap_or_else(|e| panic!("env var {var}: {e}"))
}

#[proc_macro]
pub fn runtime_linked_image(input: TokenStream) -> TokenStream {
    let mut base = None;
    let mut paths: Vec<String> = Vec::new();
    // a literal forwarded through a `macro_rules!` fragment arrives
    // wrapped in an invisible group
    let flat = input.into_iter().flat_map(|tt| match tt {
        TokenTree::Group(g) if g.delimiter() == Delimiter::None => g.stream().into_iter().collect(),
        other => vec![other],
    });
    for tt in flat {
        match tt {
            TokenTree::Literal(lit) if base.is_none() => {
                base = Some(parse_base(&lit.to_string()));
            }
            TokenTree::Literal(lit) => paths.push(env_path(&lit.to_string())),
            TokenTree::Punct(p) if p.as_char() == ',' => {}
            other => panic!("runtime_linked_image!: unexpected token {other}"),
        }
    }
    let base = base.expect("runtime_linked_image!: missing base address");
    let archives: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| {
            std::fs::read(p).unwrap_or_else(|e| panic!("runtime_linked_image!: read {p}: {e}"))
        })
        .collect();
    let borrowed: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    let runtime = prestripped_runtime_image(&borrowed)
        .unwrap_or_else(|e| panic!("runtime_linked_image!: {e:?}"));
    let image = build_rtlinked_image(&runtime, base)
        .unwrap_or_else(|e| panic!("runtime_linked_image!: {e:?}"));

    let mut src = String::from("{\n");
    for p in &paths {
        src.push_str(&format!("const _: &[u8] = include_bytes!({p:?});\n"));
    }
    src.push_str(&format!(
        "#[repr(C, align({SEGMENT_ALIGN}))]\nstruct Aligned([u8; {}]);\n\
         static IMAGE: Aligned = Aligned(*{});\n&IMAGE.0\n}}",
        image.len(),
        Literal::byte_string(&image),
    ));
    src.parse()
        .expect("runtime_linked_image!: generated expression parses")
}

#[proc_macro]
pub fn runtime_image(input: TokenStream) -> TokenStream {
    let mut paths: Vec<String> = Vec::new();
    let flat = input.into_iter().flat_map(|tt| match tt {
        TokenTree::Group(g) if g.delimiter() == Delimiter::None => g.stream().into_iter().collect(),
        other => vec![other],
    });
    for tt in flat {
        match tt {
            TokenTree::Literal(lit) => paths.push(env_path(&lit.to_string())),
            TokenTree::Punct(p) if p.as_char() == ',' => {}
            other => panic!("runtime_image!: unexpected token {other}"),
        }
    }
    let archives: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("runtime_image!: read {p}: {e}")))
        .collect();
    let borrowed: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    let image =
        prestripped_runtime_image(&borrowed).unwrap_or_else(|e| panic!("runtime_image!: {e:?}"));

    let mut src = String::from("{\n");
    for p in &paths {
        src.push_str(&format!("const _: &[u8] = include_bytes!({p:?});\n"));
    }
    src.push_str(&format!("{}\n}}", Literal::byte_string(&image)));
    src.parse()
        .expect("runtime_image!: generated expression parses")
}
