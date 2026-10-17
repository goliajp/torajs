//! Source-level checks of the null narrows at points control reaches
//! from more than one place: a `switch` clause entered from the head
//! and from the clause above, a closure called after a write, and the
//! condition of a `do..while` reached by `continue`.

fn check_src(src: &str) -> Result<(), String> {
    let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
    let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
    crate::ast::lift_arrow_fns(&mut ast);
    crate::check::check(&ast).map(|_| ())
}

fn refused(body: &str) {
    let src = format!("{PRELUDE}{body}");
    let err = check_src(&src).expect_err(&src);
    assert!(err.to_lowercase().contains("null"), "{err}\n{src}");
}

fn accepted(body: &str) {
    let src = format!("{PRELUDE}{body}");
    let r = check_src(&src);
    assert!(r.is_ok(), "{r:?}\n{src}");
}

const PRELUDE: &str = "type Node = { v: number };\n\
     function mk(): Node { return { v: 7 } }\n\
     function g(n: Node): number { return n.v }\n";

#[test]
fn a_switch_clause_is_also_entered_from_the_head() {
    refused(
        "function h(q: Node | null, k: number): number {\n\
         switch (k) { case 1: if (q === null) q = mk(); case 2: return g(q); }\n\
         return 0 }\n",
    );
    refused(
        "function h(q: Node | null, k: number): number {\n\
         switch (k) { case 1: if (q === null) return 0; case 2: return g(q); }\n\
         return 0 }\n",
    );
    // a default in the middle is entered from the head and from above
    refused(
        "function h(q: Node | null, k: number): number {\n\
         switch (k) { case 1: if (q === null) return 0; default: return g(q); case 2: return 1; }\n\
         }\n",
    );
    refused(
        "function h(q: Node | null, k: number): number {\n\
         switch (k) { default: if (q === null) return 0; case 2: return g(q); }\n\
         }\n",
    );
}

#[test]
fn a_switch_clause_keeps_what_every_way_in_agrees_on() {
    // narrowed before the switch: every clause sees it
    accepted(
        "function h(q: Node | null, k: number): number {\n\
         if (q === null) return 0;\n\
         switch (k) { case 1: k++; case 2: return g(q); }\n\
         return 0 }\n",
    );
    // a narrow inside one clause holds for the rest of that clause
    accepted(
        "function h(q: Node | null, k: number): number {\n\
         switch (k) { case 1: if (q === null) return 0; return g(q); case 2: return 1; }\n\
         return 0 }\n",
    );
    // the clause above cannot fall through: the head is the only way in
    refused(
        "function h(q: Node | null, k: number): number {\n\
         switch (k) { case 1: if (q === null) return 0; break; case 2: return g(q); }\n\
         return 0 }\n",
    );
}

#[test]
fn a_closure_does_not_keep_a_narrow_a_later_write_undoes() {
    refused(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         if (q === null) q = mk();\n\
         const f = (): number => g(q);\n\
         q = null;\n\
         return f() }\n",
    );
    refused(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const f = (): number => g(q);\n\
         q = null;\n\
         return f() }\n",
    );
    refused(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         if (q === null) return 0;\n\
         const f = (): number => g(q);\n\
         q = null;\n\
         return f() }\n",
    );
    refused(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         q = mk();\n\
         const f = (): number => g(q);\n\
         q = null;\n\
         return f() }\n",
    );
}

#[test]
fn a_closure_keeps_a_narrow_nothing_writes_again() {
    accepted(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const f = (): number => g(q);\n\
         return f() }\n",
    );
    accepted(
        "function h(a: Node | null): number {\n\
         const q: Node | null = a;\n\
         if (q === null) return 0;\n\
         const f = (): number => g(q);\n\
         return f() }\n",
    );
    // a write to a same-named binding of another function is not one
    accepted(
        "function w(): number { let q: Node | null = mk(); q = null; return 0 }\n\
         function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         if (q === null) return 0;\n\
         const f = (): number => g(q);\n\
         return f() }\n",
    );
    // a closure that does not need the narrow sees the write
    accepted(
        "function h(a: Node | null): boolean {\n\
         let q: Node | null = a;\n\
         if (q === null) q = mk();\n\
         const f = (): boolean => q === null;\n\
         q = null;\n\
         return f() }\n",
    );
}

#[test]
fn a_closure_does_not_keep_a_narrow_a_loop_or_guard_lets_out() {
    // created in a loop that writes the binding before it
    refused(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         const fs: (() => number)[] = [];\n\
         for (let i = 0; i < 2; i++) { q = null; if (q === null) q = mk(); fs.push((): number => g(q)) }\n\
         return fs[0]() }\n",
    );
    // created under a guard, called after a write past the guard
    refused(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         let f = (): number => 0;\n\
         if (q !== null) { f = (): number => g(q) }\n\
         q = null;\n\
         return f() }\n",
    );
    // written inside another closure
    refused(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const f = (): number => g(q);\n\
         const z = (): void => { q = null };\n\
         z();\n\
         return f() }\n",
    );
}

#[test]
fn a_do_while_condition_is_also_reached_by_continue() {
    refused(
        "function h(q: Node | null, k: number): number {\n\
         let s = 0;\n\
         do { if (q === null) continue; s++ } while (g(q) > 0 && --k > 0);\n\
         return s }\n",
    );
    accepted(
        "function h(q: Node | null, k: number): number {\n\
         let s = 0;\n\
         do { if (q === null) return 0; s++ } while (g(q) > 0 && --k > 0);\n\
         return s }\n",
    );
}

const ONE: &str = "function one(x: Node | null): number { return 1 }\n";

#[test]
fn a_case_value_write_reaches_the_clauses_after_it() {
    let lazy = "let q: Node | null = a;\nif (q === null) q = mk();\n";
    refused(&format!(
        "{ONE}function h(a: Node | null, k: number): number {{\n{lazy}\
         switch (k) {{ case one(q = null): return g(q); }}\nreturn 0 }}\n"
    ));
    // a later clause is entered after every case value above it ran
    refused(&format!(
        "{ONE}function h(a: Node | null, k: number): number {{\n{lazy}\
         switch (k) {{ case one(q = null): return 0; case 2: return g(q); }}\nreturn 0 }}\n"
    ));
    refused(&format!(
        "{ONE}function h(a: Node | null, k: number): number {{\n{lazy}\
         switch (k) {{ default: return g(q); case one(q = null): return 0; }}\n}}\n"
    ));
    // a clause above the write is not entered through it
    accepted(&format!(
        "{ONE}function h(a: Node | null, k: number): number {{\n{lazy}\
         switch (k) {{ case 2: return g(q); case one(q = null): return 0; }}\nreturn 0 }}\n"
    ));
}

#[test]
fn a_closure_keeps_a_narrow_no_later_write_undoes() {
    // lazy initialization, nothing written after the closure
    accepted(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         if (q === null) q = mk();\n\
         const f = (): number => g(q);\n\
         return f() }\n",
    );
    // a null write before the closure is not one after it
    accepted(
        "function h(): number {\n\
         let q: Node | null = null;\n\
         q = mk();\n\
         return [1].map((x: number): number => g(q))[0] }\n",
    );
    // a later write of a non-null value keeps it non-null
    accepted(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         if (q === null) q = mk();\n\
         const f = (): number => g(q);\n\
         q = mk();\n\
         return f() }\n",
    );
    // a loop that writes only non-null values
    accepted(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const fs: (() => number)[] = [];\n\
         for (let i = 0; i < 2; i++) { q = mk(); fs.push((): number => g(q)) }\n\
         return fs[0]() }\n",
    );
}

#[test]
fn a_closure_in_a_closure_sees_writes_around_both() {
    // created inside another closure, which escapes before the write
    refused(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const w = (): (() => number) => {\n\
         if (q === null) return (): number => 0;\n\
         return (): number => g(q) };\n\
         const f = w();\n\
         q = null;\n\
         return f() }\n",
    );
    // written inside a closure created before it
    refused(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const z = (): void => { q = null };\n\
         const f = (): number => g(q);\n\
         z();\n\
         return f() }\n",
    );
}

#[test]
fn a_closure_first_reads_a_capture_at_its_declared_type() {
    // the body checks against `Node | null`: the later null store stays
    accepted(
        "function h(a: Node | null): number {\n\
         let q: Node | null = a;\n\
         if (q === null) q = mk();\n\
         const f = (): number => q !== null ? g(q) : 0;\n\
         q = null;\n\
         return f() }\n",
    );
    // only the capture that needs the narrow keeps it
    accepted(
        "function h(a: Node | null, b: Node | null): number {\n\
         let q: Node | null = a;\n\
         let r: Node | null = b;\n\
         if (q === null) q = mk();\n\
         if (r === null) r = mk();\n\
         const f = (): number => g(q) + (r === null ? 1 : 0);\n\
         r = null;\n\
         return f() }\n",
    );
    // a narrow the body needs, undone by a null store in the body
    refused(
        "function h(a: Node): number {\n\
         let q: Node | null = a;\n\
         const f = (): number => { const r = g(q); q = null; return r };\n\
         f();\n\
         return f() }\n",
    );
}

#[test]
fn a_closure_keeps_a_narrow_a_loop_declaring_the_binding_renews() {
    // a binding of each iteration is not written by the next one
    accepted(
        "function h(xs: (Node | null)[]): number {\n\
         const fs: (() => number)[] = [];\n\
         for (const x of xs) {\n\
         let q: Node | null = null;\n\
         q = x;\n\
         if (q === null) q = mk();\n\
         fs.push((): number => g(q)) }\n\
         return fs[0]() }\n",
    );
    // one binding across iterations is
    refused(
        "function h(xs: (Node | null)[]): number {\n\
         let q: Node | null = null;\n\
         const fs: (() => number)[] = [];\n\
         for (const x of xs) { q = x; if (q === null) continue; fs.push((): number => g(q)) }\n\
         return fs[0]() }\n",
    );
}

#[test]
fn a_member_narrow_does_not_reach_into_a_closure() {
    refused(
        "type O = { f: Node | null };\n\
         const o: O = { f: mk() };\n\
         let c = (): number => 0;\n\
         if (o.f !== null) { c = (): number => g(o.f) }\n\
         o.f = null;\n\
         c();\n",
    );
    // a write in a closure body retires the narrow around it
    refused(
        "type O = { f: Node | null };\n\
         function h(o: O): number {\n\
         if (o.f !== null) { const z = (): void => { o.f = null }; return g(o.f) }\n\
         return 0 }\n",
    );
    accepted(
        "type O = { f: Node | null };\n\
         function h(o: O): number {\n\
         if (o.f !== null) { const z = (): number => 1; return g(o.f) + z() }\n\
         return 0 }\n",
    );
}
