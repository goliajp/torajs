//! Which classes the desugar lets stand in for an interface, and what
//! the checker then admits.

use torajs_core::{ast::Ast, lexer, parser};

fn desugared(src: &str) -> Ast {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    super::super::run_ast_prelude(&mut ast).expect("prelude");
    super::super::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    ast
}

fn witnesses(src: &str, iface: &str) -> Vec<String> {
    let ast = desugared(src);
    ast.iface_slots
        .witnesses
        .get(iface)
        .map(|cs| cs.iter().cloned().collect())
        .unwrap_or_default()
}

fn checked(src: &str) -> Result<(), String> {
    torajs_core::check::check(&desugared(src)).map(|_| ())
}

const SHAPES: &str = "interface Shape { area(): number }
class Sq { s: number; constructor(s: number) { this.s = s } area(): number { return this.s * this.s } }
class Ci { r: number; constructor(r: number) { this.r = r } area(): number { return 3 * this.r } }
";

#[test]
fn every_class_declaring_the_methods_is_a_witness() {
    assert_eq!(witnesses(SHAPES, "Shape"), ["Ci", "Sq"]);
    let ast = desugared(SHAPES);
    assert_eq!(ast.iface_slots.slot_of(0, "Shape", "area"), Some(0));
}

#[test]
fn a_method_answering_another_type_is_no_witness() {
    let src = format!("{SHAPES}class Label {{ area(): string {{ return \"x\" }} }}\n");
    assert_eq!(witnesses(&src, "Shape"), ["Ci", "Sq"]);
}

#[test]
fn a_missing_member_excludes_the_class_unless_it_is_optional() {
    let src = "interface P { id: string; init?(): void; run(x: number): number }
class A { id: string; constructor() { this.id = \"a\" } run(x: number): number { return x } }
class B { id: string; constructor() { this.id = \"b\" } }
";
    assert_eq!(witnesses(src, "P"), ["A"]);
}

#[test]
fn a_nullable_member_is_not_an_optional_one() {
    let src = "interface N { name(): string; next: N | null }
class K { name(): string { return \"k\" } }
";
    assert!(witnesses(src, "N").is_empty());
}

#[test]
fn a_conditional_under_a_class_takes_only_its_own_instances() {
    let src = "class A { x = 1; up(): number { return this.x } }
class Sub extends A { y = 2 }
class B { pad = \"p\"; x = 10; up(): number { return this.x } }
const n = process.argv.length;
";
    assert!(checked(&format!("{src}const a: A = n > 9 ? new Sub() : new A();\n")).is_ok());
    let err = checked(&format!("{src}const a: A = n > 9 ? new B() : new A();\n")).unwrap_err();
    assert!(err.contains("conditional branch"), "{err}");
}

/// The slots the lowering sees once the check settled which
/// interfaces a class instance flows into.
fn settled(src: &str) -> Result<Vec<String>, String> {
    let artifacts = torajs_core::check::check_with_arity(&desugared(src))?;
    let slots = &artifacts.5.mono_ast.iface_slots;
    let mut out: Vec<String> = slots.witnesses.keys().cloned().collect();
    out.sort();
    Ok(out)
}

#[test]
fn a_data_interface_every_class_lays_out_first_keeps_its_field_reads() {
    let src = "interface P { x: number }
class C { x: number; y: number; constructor() { this.x = 1; this.y = 2 } }
const p: P = new C();
console.log(p.x);
";
    assert_eq!(settled(src), Ok(vec![]));
    let by_name = "interface P { y: number }
class C { x: number; y: number; constructor() { this.x = 1; this.y = 2 } }
const p: P = new C();
console.log(p.y);
";
    assert_eq!(witnesses(by_name, "P"), ["C"]);
    assert_eq!(settled(by_name), Ok(vec!["P".to_string()]));
}

#[test]
fn an_interface_no_class_instance_flows_into_keeps_no_slots() {
    let src = "type Vec = { x: number; y: number };
class Particle { id: number; x: number; y: number; constructor() { this.id = 0; this.x = 1; this.y = 2 } }
function len2(v: Vec): number { return v.x * v.x + v.y * v.y }
const p = new Particle();
console.log(len2({ x: 3, y: 4 }), p.id);
";
    assert_eq!(witnesses(src, "Vec"), ["Particle"]);
    assert_eq!(settled(src), Ok(vec![]));
}

#[test]
fn a_class_instance_reached_through_another_interface_takes_its_row_there() {
    // `y = 2` under `y?: number`: a loose row, read where C keeps it
    let src = "interface A { y: number; m(): number }
interface B { y?: number }
class C { z = 1; y = 2; m() { return 3 } }
const a: A = new C();
const b: B = a;
console.log(b.y);
";
    assert_eq!(witnesses(src, "B"), ["C"]);
    assert_eq!(settled(src), Ok(vec!["A".to_string(), "B".to_string()]));
    let fits = "interface A { x: number; m(): number }
interface B { x: number }
class C { k = 0; x = 5; m() { return 1 } }
const a: A = new C();
const b: B = a;
console.log(b.x);
";
    assert_eq!(settled(fits), Ok(vec!["A".to_string(), "B".to_string()]));
}

#[test]
fn a_class_declaring_a_member_narrower_stands_behind_the_interface() {
    // `n: N` under `n: N | null` is stored alike: read in place
    let alone = "class N { v = 1 }
interface H { n: N | null; tag: string }
class C { n: N = new N(); tag = \"c\"; extra = 1 }
function read(h: H): string { return h.tag }
console.log(read(new C()));
";
    assert_eq!(witnesses(alone, "H"), ["C"]);
    assert_eq!(settled(alone), Ok(vec![]));
    let mixed = "class N { v = 1 }
interface H { n: N | null; tag: string }
class C { n: N = new N(); tag = \"c\"; extra = 1 }
class D { k = 0; n: N | null = null; tag = \"d\" }
function read(h: H): string { return h.tag }
console.log(read(new C()), read(new D()));
";
    assert_eq!(settled(mixed), Ok(vec!["H".to_string()]));
    // a `number` under `number | null` is stored otherwise
    let retyped = "interface H { v: number | null }
class C { v: number = 2 }
function read(h: H): number | null { return h.v }
console.log(read(new C()));
";
    assert_eq!(settled(retyped), Ok(vec!["H".to_string()]));
    // the structural rules still refuse a member of another type
    let other = "interface H { v: number | null }
class C { v: string = \"x\" }
const h: H = new C();
";
    assert!(checked(other).is_err());
}

fn field_ann(ast: &Ast, class: &str, key: &str) -> String {
    torajs_core::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .find_map(|s| match s {
            torajs_core::ast::Stmt::TypeDecl { name, fields, .. } if name == class => fields
                .iter()
                .find(|(n, _)| *n == *key)
                .map(|(_, a)| a.clone()),
            _ => None,
        })
        .expect("class declares the field")
}

#[test]
fn a_retyped_field_written_through_another_receiver_is_held_as_any() {
    let decls = "interface P { x: number | string }
class B { x = 1 }
class C extends B { y = 2 }
class Lone { x = 3 }
const p: P = new C();
";
    let read = desugared(&format!("{decls}console.log(p.x);\n"));
    assert_eq!(field_ann(&read, "C", "x"), "number");
    let written = desugared(&format!("{decls}p.x = \"s\";\n"));
    for c in ["B", "C"] {
        assert_eq!(field_ann(&written, c, "x"), "any", "{c}");
    }
    // `Lone` holds a slot of its own, and the write may as well be one
    // through a `P` it stands behind
    assert_eq!(field_ann(&written, "Lone", "x"), "any");
    let own = desugared(&format!(
        "{decls}class D {{ x = 4; set() {{ this.x = 5 }} }}\n"
    ));
    assert_eq!(field_ann(&own, "D", "x"), "number");
}

#[test]
fn a_subclass_without_a_row_takes_its_ancestors_witness_away() {
    let src =
        format!("{SHAPES}class Big extends Sq {{ area(k: number): number {{ return k }} }}\n");
    assert_eq!(witnesses(&src, "Shape"), ["Ci"]);
}

#[test]
fn a_witness_instance_fits_the_interface_and_another_class_does_not() {
    let fits = format!("{SHAPES}const s: Shape = new Sq(2);\nconsole.log(s.area());\n");
    assert!(checked(&fits).is_ok(), "{:?}", checked(&fits));
    let other =
        format!("{SHAPES}class O {{ w(): number {{ return 1 }} }}\nconst s: Shape = new O();\n");
    assert!(checked(&other).is_err());
}

#[test]
fn an_array_element_that_only_matched_its_first_sibling_is_refused() {
    // `O` and `Sq2` have the same (empty) data layout, so the literal
    // types as `Sq2[]`; `O` has no `area`.
    let src = format!(
        "{SHAPES}class Sq2 {{ area(): number {{ return 1 }} }}\nclass O {{ w(): number {{ return 2 }} }}\nconst xs: Shape[] = [new Sq2(), new O()];\n"
    );
    let err = checked(&src).expect_err("the second element is no Shape");
    assert!(err.contains("array element 1"), "{err}");
}

#[test]
fn an_unfit_array_element_is_refused_in_every_typed_position() {
    let base = "interface Sh { area(): number }
class A { area() { return 1 } }
class U { q = 5 }
";
    let tails = [
        "function sum(xs: Sh[]): number { return xs.length }\nsum([new A(), new U()]);\n",
        "function mk(): Sh[] { return [new A(), new U()] }\nmk();\n",
        "class Box { items: Sh[] = [] }\nconst b = new Box();\nb.items = [new A(), new U()];\n",
    ];
    for t in tails {
        let err = checked(&format!("{base}{t}")).expect_err(t);
        assert!(err.contains("array element 1"), "{t}: {err}");
    }
}
