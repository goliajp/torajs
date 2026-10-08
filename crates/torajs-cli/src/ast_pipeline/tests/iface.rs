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
fn a_data_interface_every_class_lays_out_first_keeps_its_field_reads() {
    let src = "interface P { x: number }
class C { x: number; y: number; constructor() { this.x = 1; this.y = 2 } }
";
    assert!(desugared(src).iface_slots.is_empty());
    let by_name = "interface P { y: number }
class C { x: number; y: number; constructor() { this.x = 1; this.y = 2 } }
";
    assert_eq!(witnesses(by_name, "P"), ["C"]);
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
