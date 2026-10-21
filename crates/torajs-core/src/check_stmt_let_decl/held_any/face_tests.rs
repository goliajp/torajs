//! A binding that holds an `any` value keeps its declared type for the
//! TypeScript rules: what tsc refuses through the declared type is
//! refused, what it accepts still compiles.

fn check_src(src: &str) -> Result<(), String> {
    let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
    let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
    crate::ast::lift_arrow_fns(&mut ast);
    crate::check::check(&ast).map(|_| ())
}

fn refused(src: &str, needle: &str) {
    let err = check_src(src).expect_err("program must be refused");
    assert!(err.contains(needle), "{err}");
}

fn accepted(src: &str) {
    assert!(check_src(src).is_ok(), "{:?}", check_src(src));
}

const HELD_NUMBER: &str = "const x: any = \"str\";\nconst n: number = x;\n";

#[test]
fn a_held_scalar_flows_out_at_its_declared_type() {
    refused(
        &format!("{HELD_NUMBER}const s: string = n;\n"),
        "type mismatch on `s`: declared String, init has Number",
    );
    refused(
        &format!("{HELD_NUMBER}function g(a: string) {{ return a; }}\ng(n);\n"),
        "argument 0: expected String, got Number",
    );
    refused(
        "const x: any = \"str\";\nfunction h(): string { const n: number = x; return n; }\n",
        "return type mismatch: function expects String, got Number",
    );
    refused(
        &format!("{HELD_NUMBER}const s: string = n + 1;\n"),
        "init has Number",
    );
    refused(
        &format!("{HELD_NUMBER}const m: number = n + \"\";\n"),
        "init has String",
    );
}

#[test]
fn a_held_scalar_reads_only_its_declared_members() {
    refused(
        &format!("{HELD_NUMBER}console.log(n.toUpperCase());\n"),
        "no member `.toUpperCase` on type Number",
    );
    refused(
        &format!("{HELD_NUMBER}console.log(n.foo);\n"),
        "no member `.foo` on type Number",
    );
    refused(
        "const x: any = 1;\nconst s: string = x;\nconst b: boolean = s.length;\n",
        "init has Number",
    );
}

#[test]
fn a_store_into_a_held_scalar_fits_its_declared_type() {
    refused(
        "const x: any = 3;\nlet w: number = x;\nw += \"s\";\n",
        "type mismatch assigning to `w`: declared Number, value is String",
    );
    refused(
        "const str: any = \"s\";\nlet gl: string = str;\nfunction f() { gl = 5; }\nf();\n",
        "type mismatch assigning to `gl`: declared String, value is Number",
    );
    refused(
        &format!("{HELD_NUMBER}let t: string = \"\";\nt = n;\n"),
        "type mismatch assigning to `t`: declared String, value is Number",
    );
}

#[test]
fn what_tsc_accepts_still_compiles() {
    accepted(&format!(
        "{HELD_NUMBER}const k: number = n + 1;\nconst t: string = \"\" + n;\n\
         const u: number | string = n;\nconsole.log(n.toFixed(1), k, t, u);\n\
         function g(a: number): number {{ return a; }}\n\
         function h(): number {{ return n; }}\nconsole.log(g(n), h());\n\
         let w: number = x;\nw = w * 2;\nw += 1;\nw = n;\n"
    ));
    accepted(
        "const x: any = 1;\nconst s: string = x;\nconst m: number = s.length;\n\
         console.log(s.toUpperCase(), m);\n",
    );
}
