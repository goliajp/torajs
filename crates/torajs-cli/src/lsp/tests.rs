use super::compute_diagnostics;

#[test]
fn a_type_declared_in_a_function_is_scoped_there() {
    let uri: lsp_types::Uri = "file:///tmp/scoped.ts".parse().expect("uri");
    let src = "type K = string;\n\
               function f(): number { type K = number; const x: K = 41; return x + 1; }\n\
               const s: K = \"a\";\n\
               console.log(f(), s);\n";
    let msgs: Vec<String> = compute_diagnostics(&uri, src)
        .into_iter()
        .map(|d| d.message)
        .collect();
    assert!(msgs.is_empty(), "{msgs:?}");
}
