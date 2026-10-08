//! Source-level checks of the union rules: what a union slot refuses,
//! what a narrow admits, and what a loose binding still admits — and of
//! the dictionary rules that ride the same lane: what a write, an
//! initializer or an argument for an index-signature slot must fit.

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

#[test]
fn a_value_outside_every_member_is_refused() {
    refused("let x: string | number = true;\n", "type mismatch");
    refused(
        "function f(v: string | number): string { return \"\" + v; }\nf(true);\n",
        "not assignable",
    );
}

#[test]
fn a_member_missing_on_one_member_is_refused() {
    refused(
        "function f(v: string | number): number { return v.length; }\n",
        "does not exist on every member",
    );
    accepted("function f(v: string | any[]): number { return v.length; }\n");
}

#[test]
fn typeof_narrows_each_branch() {
    accepted(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { return v.toUpperCase(); }\n\
         return v.toFixed(1);\n}\n",
    );
    refused(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { return v.toFixed(1); }\n\
         return \"\";\n}\n",
        "does not exist on every member",
    );
}

#[test]
fn an_unmodeled_guard_loosens_the_binding() {
    accepted(
        "function f(v: string | string[]): string {\n\
         if (Array.isArray(v)) return v.join(\"+\");\n\
         return v;\n}\n",
    );
    // a name on no member at all stays refused
    refused(
        "function f(v: string | number): number {\n\
         if (v === \"x\") return v.nope;\n\
         return 0;\n}\n",
        "does not exist on every member",
    );
}

#[test]
fn an_assignment_in_a_narrow_checks_the_declaration() {
    accepted(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { v = 5; }\n\
         return \"\" + v;\n}\n",
    );
    // a same-named binding elsewhere keeps its own declaration
    accepted(
        "function a(v: string | number): string {\n\
         if (typeof v === \"string\") { return v; }\n\
         return \"\";\n}\n\
         function b(v: string | number | boolean): string {\n\
         v = true;\n\
         return \"\" + v;\n}\n",
    );
    // an assignment in a narrow reaches the read after the branch
    accepted(
        "function f(v: string | number): string {\n\
         if (typeof v === \"string\") { v = 5; }\n\
         return v.toFixed(1);\n}\n",
    );
}

#[test]
fn an_optional_value_fits_a_union_with_undefined() {
    accepted(
        "function f(v: string | number | undefined): string { return \"\" + v; }\n\
         function g(s?: string): string { return f(s); }\n",
    );
    refused(
        "function f(v: string | number): string { return \"\" + v; }\n\
         function g(s?: string): string { return f(s); }\n",
        "not assignable",
    );
}

#[test]
fn an_initializer_narrows_the_declared_union() {
    accepted("let x: string | number = \"hi\";\nconsole.log(x.toUpperCase());\n");
    refused(
        "let x: string | number = \"hi\";\nconsole.log(x.toFixed(1));\n",
        "does not exist on every member",
    );
    // a typeof guard the initializer's narrow leaves empty still sees
    // the declared member a loop's back edge brings in
    accepted(
        "let x: string | number = \"a\";\n\
         for (let i = 0; i < 2; i++) {\n\
         if (typeof x === \"number\") { console.log(x.toFixed(1)); }\n\
         x = i;\n}\n",
    );
    // a closure sees the declared members too
    accepted(
        "let x: string | number = \"a\";\n\
         const f = (): string => typeof x === \"number\" ? x.toFixed(1) : x;\n\
         x = 2;\nconsole.log(f());\n",
    );
}

#[test]
fn a_loose_mark_stays_on_its_own_binding() {
    refused(
        "function a(v: string | number): number {\n\
         if (v === \"x\") return 1;\n\
         return 0;\n}\n\
         function b(v: string | number): number { return v.length; }\n",
        "does not exist on every member",
    );
}

#[test]
fn guards_held_elsewhere_loosen_the_binding() {
    // class accessors and `switch (true)` need the desugar passes this
    // harness skips; the union-*.ts conformance cases cover them
    accepted(
        "function b(y: string | number): number {\n\
         const isS = typeof y === \"string\";\n\
         if (isS) return y.length;\n\
         return 0;\n}\n",
    );
    accepted(
        "function assertS(v: unknown): asserts v is string {\n\
         if (typeof v !== \"string\") throw \"no\";\n}\n\
         function c(y: string | number): number { assertS(y); return y.length; }\n",
    );
}

#[test]
fn a_dictionary_write_must_fit_its_value_type() {
    refused(
        "const d: Record<string, number> = {};\nd.a = \"s\";\n",
        "dictionary's value type",
    );
    refused(
        "const d: { [k: string]: number } = {};\nd[\"a\"] = \"s\";\n",
        "dictionary's value type",
    );
    accepted(
        "const d: Record<string, number> = {};\n\
         const k = \"w\";\nd[k] = (d[k] ?? 0) + 1;\nd.x = 2;\n",
    );
    accepted("const d: Record<string, string | number> = {};\nd.a = 1;\nd.b = \"s\";\n");
}

#[test]
fn a_dictionary_slot_takes_what_fits_its_value_type() {
    refused(
        "const d: Record<string, number> = { a: \"x\" };\n",
        "type mismatch",
    );
    refused(
        "function f(d: Record<string, number>): number { return 0; }\nf({ a: \"x\" });\n",
        "not assignable",
    );
    refused(
        "const d: Record<string, number> = {};\nconst e: Record<string, string> = d;\n",
        "type mismatch",
    );
    accepted("const r: Record<\"a\" | \"b\", number> = { a: 1, b: 2 };\n");
    accepted("const a: { [i: number]: string } = [\"p\", \"q\"];\n");
    accepted("const n: Record<string, Record<string, number>> = {};\nn.x = { y: 1 };\n");
}

#[test]
fn a_named_member_beside_an_index_signature_must_fit_it() {
    accepted(
        "interface E { name: string; [k: string]: string }\n\
         const e: E = { name: \"n\", r: \"x\" };\n",
    );
    refused(
        "interface E { name: number; [k: string]: string }\nconst e: E = { name: 1 };\n",
        "property `name` of type `number` is not assignable to the `string` index type `string` (TS2411) at 14",
    );
    // a `number` signature covers only the numeric-like names
    accepted(
        "interface StringArray { [i: number]: string; length: number }\n\
         const s: StringArray = { length: 0 };\n",
    );
    accepted(
        "interface S { [i: number]: string; length: number }\n\
         const s: S = { length: 0 };\ns.length = 1;\ns[0] = \"a\";\n",
    );
    refused(
        "interface S { [i: number]: string; length: number }\n\
         const s: S = { length: 0 };\ns[0] = 1;\n",
        "dictionary's value type",
    );
    accepted("interface Base { a: number }\ninterface D extends Base { [k: string]: number }\n");
    refused(
        "interface Base { a: string }\ninterface D extends Base { [k: string]: number }\n",
        "property `a` inherited from `Base` is not assignable",
    );
}

#[test]
fn a_dictionary_key_must_be_a_property_key_type() {
    refused(
        "const r: Record<boolean, number> = {};\n",
        "type `boolean` does not satisfy the constraint `string | number | symbol` of `Record` (TS2344) at 16",
    );
    refused(
        "const r: { [k: boolean]: number } = {};\n",
        "an index signature parameter type must be `string`, `number`, `symbol`, or a template literal type, not `boolean` (TS1268) at 12",
    );
    accepted("type Key = string;\nconst d: Record<Key, boolean> = {};\n");
    accepted(
        "function get<V>(r: Record<string, V>, k: string): V { return r[k]; }\n\
         const n: number = get({ a: 1, b: 2 }, \"a\");\n",
    );
    accepted(
        "function pick<K extends string>(r: Record<K, number>, k: K): number { return r[k]; }\n\
         const m: number = pick({ a: 1 }, \"a\");\n",
    );
    // a line break separates members, and ends an array suffix
    accepted(
        "interface E {\n  name: string\n  [k: string]: string\n}\n\
         const e: E = { name: \"n\" };\n",
    );
    accepted("interface P {\n  a: number\n  b: string[]\n}\nconst p: P = { a: 1, b: [] };\n");
}
