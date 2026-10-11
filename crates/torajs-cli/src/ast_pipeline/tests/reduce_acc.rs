//! What the reducer accumulator check refuses and what it lets through,
//! after the full AST pipeline ran over the program.

use torajs_core::{lexer, parser};

fn check_src(src: &str) -> Result<(), String> {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    super::super::run_ast_prelude(&mut ast).expect("prelude");
    super::super::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    torajs_core::check::check(&ast).map(|_| ())
}

const P: &str = "const ys = [1, 2, 3];\nconst ws = [\"a\", \"bb\"];\n";

fn refused(body: &str, says: &str) {
    let err = check_src(&format!("{P}{body}")).expect_err(body);
    assert!(err.contains(says), "{body}: {err}");
}

fn accepted(body: &str) {
    let r = check_src(&format!("{P}{body}"));
    assert!(r.is_ok(), "{body}: {r:?}");
}

#[test]
fn an_accumulator_that_cannot_take_the_seed_is_refused() {
    let says = "accumulator parameter is String but the initial value is Number";
    refused("ys.reduce((acc: string, x: number) => acc + x, 0);\n", says);
    refused(
        "ys.reduceRight((acc: string, x: number) => acc + x, 0);\n",
        says,
    );
    refused(
        "ys.reduce((acc: string, ...xs: any[]) => acc + xs.length, 0);\n",
        says,
    );
    refused(
        "ws.reduce((acc: number, x: string) => acc + x.length, \"\");\n",
        "accumulator parameter is Number but the initial value is String",
    );
}

#[test]
fn without_a_seed_the_accumulator_must_take_the_element() {
    let says = "accumulator parameter is String but with no initial value";
    refused("ys.reduce((acc: string, x: number) => acc + x);\n", says);
    refused(
        "ys.reduce((acc: string, ...xs: any[]) => acc + xs.length);\n",
        says,
    );
}

#[test]
fn the_return_must_fit_the_accumulator() {
    refused(
        "ys.reduce((acc: number, x: number) => \"s\" + acc + x, 0);\n",
        "returns String but its accumulator parameter is Number",
    );
}

#[test]
fn the_element_parameter_must_take_the_element() {
    refused(
        "ys.reduce((acc: number, x: string) => acc + x.length, 0);\n",
        "element parameter is String but the array holds Number",
    );
}

#[test]
fn typescript_accepted_reducers_pass() {
    accepted("ys.reduce((acc: number, x) => acc + x, 0);\n");
    accepted("ys.reduce((acc: number, x: number) => acc + x);\n");
    accepted("ys.reduce((acc: string, x: number) => acc + x, \"\");\n");
    accepted("ws.reduce((acc: number, w: string) => acc + w.length, 0);\n");
    accepted("ys.reduce((acc, x) => acc + x, \"\");\n");
    accepted("ys.reduce((acc: any, x) => acc + x, \"s\");\n");
    accepted("const s: any = 5;\nys.reduce((acc: string, x) => acc + x, s);\n");
    accepted("ys.reduceRight((acc: string, x) => acc + x, \"\");\n");
    accepted("ys.reduce((acc: number, ...xs: any[]) => acc + xs.length, 0);\n");
    accepted(
        "ws.reduce((acc: Record<string, number>, w) => { acc[w] = w.length; return acc; }, {});\n",
    );
}
