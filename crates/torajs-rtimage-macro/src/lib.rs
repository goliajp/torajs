//! `runtime_image!("ENV_A", "ENV_B", ...)` — each argument names an
//! env var the embedding crate's build script set to a staticlib
//! path. The macro reads those archives in the given order (link
//! search order), builds the runtime image of the members a program can
//! pull (`image::build_reachable`) and expands to it as one
//! byte-string literal; nothing is written to disk.
//!
//! Dependency tracking: the expansion also names every archive in an
//! unused `include_bytes!`, which puts the file in rustc's dep-info
//! (cargo rebuilds the crate when an archive changes) without
//! embedding it — an unused `const _` is evaluated, not emitted.

use proc_macro::{Literal, TokenStream, TokenTree};

#[proc_macro]
pub fn runtime_image(input: TokenStream) -> TokenStream {
    let mut paths: Vec<String> = Vec::new();
    for tt in input {
        match tt {
            TokenTree::Literal(lit) => {
                let s = lit.to_string();
                let var = s
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or_else(|| {
                        panic!("runtime_image!: expected a string literal, got {s}")
                    });
                let path = std::env::var(var)
                    .unwrap_or_else(|e| panic!("runtime_image!: env var {var}: {e}"));
                paths.push(path);
            }
            TokenTree::Punct(p) if p.as_char() == ',' => {}
            other => panic!("runtime_image!: unexpected token {other}"),
        }
    }
    let archives: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("runtime_image!: read {p}: {e}")))
        .collect();
    let borrowed: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    let image = torajs_rtimage::image::build_reachable(&borrowed)
        .unwrap_or_else(|e| panic!("runtime_image!: {e:?}"));

    let mut src = String::from("{\n");
    for p in &paths {
        src.push_str(&format!("const _: &[u8] = include_bytes!({p:?});\n"));
    }
    src.push_str(&format!("{}\n}}", Literal::byte_string(&image)));
    src.parse()
        .expect("runtime_image!: generated expression parses")
}
