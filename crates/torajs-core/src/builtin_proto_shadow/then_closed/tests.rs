use std::collections::HashMap;

use super::then_opening;
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

fn ast_of(src: &str) -> Ast {
    let tokens = crate::lexer::tokenize(src).unwrap();
    let mut ast = crate::parser::parse(src, &tokens).unwrap();
    crate::ast::desugar_classes(&mut ast);
    crate::ast::desugar_objlit_super(&mut ast);
    ast
}

/// The first form the proof could not clear, without types.
fn why(src: &str) -> Option<&'static str> {
    then_opening(&ast_of(src), None).map(|o| o.why)
}

/// The proof with every `as` cast's operand typed `from` and the cast
/// itself typed `to`.
fn why_cast(src: &str, from: Type, to: Type) -> Option<&'static str> {
    let ast = ast_of(src);
    let mut types: HashMap<ExprId, Type> = HashMap::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::As { expr, .. } = e {
            types.insert(*expr, from.clone());
            types.insert(ExprId(i as u32), to.clone());
        }
    }
    then_opening(&ast, Some(&types)).map(|o| o.why)
}

#[test]
fn a_program_of_known_keys_is_closed() {
    assert_eq!(why("const o = { a: 1 }; o.a = 2; console.log(o.a);"), None);
    assert_eq!(
        why("const xs = [1, 2]; xs[0] = 3; const ks = Object.keys({ b: 1 });"),
        None
    );
    assert_eq!(
        why("const p = Promise.resolve(1); p.then((v) => console.log(v));"),
        None
    );
    assert_eq!(
        why("const v = JSON.parse(\"[1]\"); console.log(JSON.stringify(v));"),
        None
    );
    assert_eq!(
        why("class K { x = 1; m() { return this.x; } } new K().m();"),
        None
    );
}

#[test]
fn a_then_spelled_anywhere_opens() {
    assert!(why("const o: any = {}; o.then = 1;").is_some());
    assert!(why("const o: any = {}; o[\"then\"] = 1;").is_some());
    assert!(why("const o = { then: 1 };").is_some());
    assert!(why("const o = { get then() { return 1; } };").is_some());
    assert!(why("class T { then(r: any) { r(1); } }").is_some());
    assert!(why("class T { static then() {} }").is_some());
    assert!(why("function then() {}").is_some());
    assert!(why("const s = \"th\" + \"en\"; console.log(\"then\");").is_some());
}

#[test]
fn a_reflective_form_however_reached_opens() {
    for src in [
        "const dp = Object.defineProperty; dp({}, \"a\", { value: 1 });",
        "const rs = Reflect.set;",
        "const O = Object; O.keys({});",
        "const { defineProperty } = Reflect;",
        "Object.defineProperty.call(null, {}, \"a\", { value: 1 });",
        "Object.assign({}, { a: 1 });",
        "const o = Object.create(null);",
        "const o = {}; Object.setPrototypeOf(o, null);",
        "const o = Object.fromEntries([[\"a\", 1]]);",
        "const d = Object.getOwnPropertyDescriptor({ a: 1 }, \"a\");",
        "const p = new Proxy({}, {});",
        "const p = new globalThis.Proxy({}, {});",
        "const P = globalThis[\"Proxy\"];",
        "globalThis.eval(\"1\");",
        "const f = new Function(\"return 1\");",
        "const F = (() => 1).constructor;",
        "const o: any = {}; o.__defineGetter__(\"a\", () => 1);",
        "const o: any = {}; o.__proto__ = null;",
        "const o = { __proto__: null };",
        "const x = JSON.parse(\"{}\", (k: any, v: any) => v);",
        "const a: any = []; const x = JSON.parse(...a);",
        "const p = Object.getPrototypeOf(Promise.resolve(1));",
        "const p = JSON.parse; const x = p(\"{}\", (k: any, v: any) => v);",
        "const pr = Object.prototype;",
        "class P2 extends Promise<number> {}",
    ] {
        assert!(why(src).is_some(), "closed: {src}");
    }
}

#[test]
fn a_computed_key_clears_only_by_its_spelled_value() {
    // a binding's value is not spelled at the key, whatever its type
    assert!(why("const o: any = {}; const k = \"a\"; o[k] = 1;").is_some());
    assert!(why("const o: any = {}; const k = \"a\" as \"a\"; o[k] = 1;").is_some());
    assert!(why("const xs = [1]; for (let i = 0; i < 1; i++) xs[i] = 2;").is_some());
    let m = "const o: any = {}; const m = \"__defineGetter__\"; o[m](\"x\", () => 1);";
    assert!(why(m).is_some());
    // literals fold
    assert_eq!(
        why("const o: any = {}; o[\"a\" + \"b\"] = 1; o[-1] = 2; o[1 + 2] = 3;"),
        None
    );
    assert_eq!(
        why("const o: any = {}; o[!0] = 1; o[2n] = 2; o[1 < 2] = 3;"),
        None
    );
    assert!(why("const o: any = {}; o[\"__pro\" + \"to__\"];").is_some());
    assert!(why("const o: any = {}; o[\"th\" + \"en\"] = 1;").is_some());
    // a number's spelling and the operand `&&` answers are not modelled
    assert!(why("const o: any = {}; o[\"a\" + 1] = 1;").is_some());
    assert!(why("const o: any = {}; o[\"a\" || \"b\"] = 1;").is_some());
}

#[test]
fn a_cast_from_any_to_an_object_type_opens() {
    let src = "const v: any = 1; const o = v as { a: number };";
    let obj = Type::Struct(Default::default());
    assert_eq!(why(src), None);
    assert!(why_cast(src, Type::Any, obj.clone()).is_some());
    // a generic body's cast is judged by its specializations' copies
    assert_eq!(why_cast(src, Type::TypeVar("T".into()), obj.clone()), None);
    assert_eq!(why_cast(src, Type::Number, obj), None);
    assert_eq!(why_cast(src, Type::Any, Type::Number), None);
}

#[test]
fn a_class_member_under_a_computed_key_opens() {
    assert!(why("class T { [\"th\" + \"en\"](r: any) { r(1); } }").is_some());
    assert!(why("const k = \"a\"; class T { get [k]() { return 1; } }").is_some());
}

#[test]
fn a_super_write_clears_only_under_a_spelled_harmless_key() {
    let objlit = |body: &str| {
        why(&format!(
            "const o = {{ m(k: string, v: any) {{ {body} }} }};"
        ))
    };
    assert!(objlit("super.then = v;").is_some());
    assert!(objlit("super[k] = v;").is_some());
    assert!(objlit("super.__proto__ = v;").is_some());
    assert_eq!(objlit("super.a = v;"), None);
    assert_eq!(objlit("super[\"a\" + \"b\"] = v;"), None);
    assert!(objlit("super[\"th\" + \"en\"] = v;").is_some());
}

#[test]
fn the_super_base_of_a_literal_method_clears_and_no_other_get_prototype_of() {
    let objlit = |body: &str| why(&format!("const o = {{ m() {{ {body} }} }};"));
    assert_eq!(objlit("return super.a;"), None);
    assert_eq!(objlit("return delete super.a;"), None);
    let user_home =
        "const __home_0 = Promise.resolve(1); const p = Object.getPrototypeOf(__home_0);";
    assert!(why(user_home).is_some());
}

#[test]
fn a_spread_into_json_parse_opens_wherever_it_stands() {
    assert!(why("const a: any = []; const x = JSON.parse(...a);").is_some());
    assert!(why("const a: any = []; const x = JSON.parse(\"{}\", ...a);").is_some());
    assert_eq!(why("const x = JSON.parse(\"{}\");"), None);
}

#[test]
fn a_global_read_off_some_object_opens() {
    for src in [
        "function g(this: any) { return this; } const G = g(); G.Object.keys({});",
        "const G: any = {}; G[\"Reflect\"].set({}, \"a\", 1);",
        "const G: any = {}; const P = G.Promise;",
        "const G: any = {}; G[\"Fun\" + \"ction\"](\"return 1\");",
    ] {
        assert!(why(src).is_some(), "closed: {src}");
    }
}
