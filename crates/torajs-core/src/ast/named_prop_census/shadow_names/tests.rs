use super::*;

fn settled(src: &str, types: impl Fn(&Ast) -> HashMap<ExprId, Type>) -> AccessorFieldNames {
    let tokens = crate::lexer::tokenize(src).expect("tokenize");
    let mut ast = crate::parser::parse(src, &tokens).expect("parse");
    let types = types(&ast);
    AccessorFieldNames::settle_shadow_writes(
        &mut ast,
        &types,
        (&HashMap::new(), &HashMap::new()),
        &ShadowedNames::default(),
    );
    ast.accessor_field_names
}

/// The type of every `Ident(name)` in the arena, and `number` for
/// every numeric literal.
fn ident_types(ast: &Ast, name: &str, t: Type) -> HashMap<ExprId, Type> {
    (ast.exprs.iter().enumerate())
        .filter_map(|(i, e)| match e {
            Expr::Ident(n) if n == name => Some((ExprId(i as u32), t.clone())),
            Expr::Number(_) => Some((ExprId(i as u32), Type::Number)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_write_through_an_any_receiver_names_its_field() {
    let n = settled("x.a = null; x.b++;", |ast| ident_types(ast, "x", Type::Any));
    assert!(n.may_shadow("a", true) && n.may_shadow("a", false));
    // a `++` writes a number: it fits an f64 slot
    assert!(!n.may_shadow("b", true) && n.may_shadow("b", false));
    assert!(!n.may_shadow("c", false));
}

#[test]
fn a_typed_store_of_a_number_keeps_the_bare_load() {
    let fs = crate::check::Type::struct_of(vec![
        (PropKey::from("a"), Type::Number),
        (PropKey::from("s"), Type::String),
    ]);
    let n = settled("p.a = 1; p.s = v;", |ast| {
        let mut t = ident_types(ast, "p", fs.clone());
        t.extend(ident_types(ast, "v", Type::Any));
        t
    });
    assert!(!n.may_shadow("a", true) && !n.may_shadow("s", false));
    let n = settled("p.a = v; p.a = null;", |ast| {
        let mut t = ident_types(ast, "p", fs.clone());
        t.extend(ident_types(ast, "v", Type::Any));
        t
    });
    assert!(n.may_shadow("a", true));
}

#[test]
fn a_computed_key_reaches_every_name_unless_it_indexes() {
    let n = settled("p[k] = v;", |ast| {
        let mut t = ident_types(ast, "p", Type::Any);
        t.extend(ident_types(ast, "v", Type::Any));
        t
    });
    assert!(n.may_shadow("anything", true));
    let n = settled("xs[i] = 1;", |ast| {
        let mut t = ident_types(ast, "xs", Type::Array(Box::new(Type::Number)));
        t.extend(ident_types(ast, "i", Type::Number));
        t
    });
    assert!(!n.may_shadow("a", false));
    // a fraction: an integer literal fits every number slot
    let n = settled("p[0] = 1.5; q[\"b\"] = 2.5;", |ast| {
        ident_types(ast, "p", Type::Any)
    });
    assert!(n.may_shadow("0", false) && n.may_shadow("b", false));
    assert!(!n.may_shadow("b", true) && !n.may_shadow("c", false));
}

#[test]
fn a_number_written_under_a_computed_key_spares_the_f64_slots() {
    let n = settled("o[w] = n;", |ast| {
        let mut t = ident_types(ast, "o", Type::Any);
        t.extend(ident_types(ast, "n", Type::Number));
        t
    });
    assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
}

#[test]
fn a_dictionary_may_alias_a_struct_cell() {
    let dict = |v: Type| Type::Dict(Box::new(Type::String), Box::new(v));
    let n = settled("counts[w] = v;", |ast| {
        let mut t = ident_types(ast, "counts", dict(Type::Any));
        t.extend(ident_types(ast, "v", Type::Any));
        t
    });
    assert!(n.may_shadow("a", true) && n.may_shadow("a", false));
    // a number keeps the f64 slots' bare loads
    let n = settled("counts[w] = n; d[\"s\"] = n;", |ast| {
        let mut t = ident_types(ast, "counts", dict(Type::Number));
        t.extend(ident_types(ast, "d", dict(Type::Number)));
        t.extend(ident_types(ast, "n", Type::Number));
        t
    });
    assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
    let n = settled("d[\"s\"] = v;", |ast| {
        let mut t = ident_types(ast, "d", dict(Type::Any));
        t.extend(ident_types(ast, "v", Type::Any));
        t
    });
    assert!(n.may_shadow("s", false) && !n.may_shadow("t", false));
}

#[test]
fn reflective_writes_reach_their_keys() {
    let n = settled(r#"Reflect.set(p, "a", v);"#, |ast| {
        ident_types(ast, "v", Type::Any)
    });
    assert!(n.may_shadow("a", true) && !n.may_shadow("b", true));
    assert!(settled("Reflect.set(p, k, v);", |_| HashMap::new()).may_shadow("zz", true));
    let n = settled("Reflect.set(p, k, 1);", |ast| {
        ident_types(ast, "k", Type::String)
    });
    assert!(!n.may_shadow("zz", true) && n.may_shadow("zz", false));
}

#[test]
fn object_assign_reaches_the_fields_of_its_sources() {
    let src = crate::check::Type::struct_of(vec![
        (PropKey::from("a"), Type::Number),
        (PropKey::from("b"), Type::Number),
    ]);
    let n = settled("Object.assign(p, q);", |ast| {
        ident_types(ast, "q", src.clone())
    });
    assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
    let n = settled("Object.assign(p, q);", |ast| {
        ident_types(ast, "q", Type::Any)
    });
    assert!(n.may_shadow("zz", true));
    let one = crate::check::Type::struct_of(vec![(PropKey::from("s"), Type::String)]);
    let n = settled("Object.assign(p, q);", |ast| {
        ident_types(ast, "q", one.clone())
    });
    assert!(n.may_shadow("s", true) && !n.may_shadow("t", false));
    // several fields of mixed kinds: each name with its own
    let mixed = crate::check::Type::struct_of(vec![
        (PropKey::from("a"), Type::Number),
        (PropKey::from("s"), Type::String),
    ]);
    let n = settled("Object.assign(p, q, r);", |ast| {
        let mut t = ident_types(ast, "q", mixed.clone());
        t.extend(ident_types(ast, "r", src.clone()));
        t
    });
    assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
    assert!(n.may_shadow("s", true) && !n.may_shadow("b", true));
    assert!(!n.may_shadow("zz", false));
}

#[test]
fn an_any_receiver_stepping_its_own_number_member_fits_the_slot() {
    let decl = "type P = { pos: number; start: number; s: string };";
    let any_o = |ast: &Ast| ident_types(ast, "o", Type::Any);
    let n = settled(
        &format!("{decl} o.pos = o.pos + 1; o.pos = o.pos - 2;"),
        any_o,
    );
    assert!(!n.may_shadow("pos", false));
    // a product may be -0, which an integer slot cannot hold
    let n = settled(&format!("{decl} o.pos = o.pos * 2;"), any_o);
    assert!(!n.may_shadow("pos", true) && n.may_shadow("pos", false));
    // another number member of the same receiver
    let n = settled(&format!("{decl} o.pos = o.start;"), any_o);
    assert!(!n.may_shadow("pos", true) && n.may_shadow("pos", false));
    // a member a layout declares a string is no number
    let n = settled(&format!("{decl} o.pos = o.s;"), any_o);
    assert!(n.may_shadow("pos", true));
    // nor is a member another write leaves holding any value
    let n = settled(
        &format!("{decl} o.start = v; o.pos = o.start + 1;"),
        |ast| {
            let mut t = any_o(ast);
            t.extend(ident_types(ast, "v", Type::Any));
            t
        },
    );
    assert!(n.may_shadow("pos", true) && n.may_shadow("start", true));
}

#[test]
fn a_number_member_is_one_the_lowering_gives_a_number_slot() {
    // the parser writes a member's literal type as its base type's
    // annotation (`k: 1` declares `k: number`), and the slot follows it
    let s = |s: &str| Type::Literal(Lit::Str(s.into()));
    for checked in [
        Type::Number,
        Type::Literal(Lit::Num(1.0)),
        Type::Literal(Lit::Num(-2.5)),
        Type::String,
        s("a"),
        Type::Boolean,
        Type::Literal(Lit::Bool(true)),
    ] {
        let base = match &checked {
            Type::Literal(l) => l.base(),
            t => t.clone(),
        };
        let ann = match base {
            Type::Number => "number",
            Type::String => "string",
            _ => "boolean",
        };
        let slot = crate::ssa_lower_parse_type::parse_struct_field_type(
            ann,
            false,
            &HashMap::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &Default::default(),
            &mut Vec::new(),
            &mut HashMap::new(),
        );
        let number_slot = matches!(
            slot,
            crate::ssa::Type::F64 | crate::ssa::Type::I64 | crate::ssa::Type::I32
        );
        assert_eq!(
            ShadowedNames::number_member(&checked),
            number_slot,
            "{checked:?}"
        );
    }
}

#[test]
fn the_checker_retypes_the_members_written_a_value_of_another_type() {
    let src = "x.b = v; x.c = 1.5; x.__state = v;";
    let tokens = crate::lexer::tokenize(src).expect("tokenize");
    let ast = crate::parser::parse(src, &tokens).expect("parse");
    let mut types = ident_types(&ast, "x", Type::Any);
    types.extend(ident_types(&ast, "v", Type::Any));
    let names = ShadowedNames::of(&ast, &types);
    assert!(names.may_mistype("b", true) && names.may_mistype("b", false));
    assert!(!names.may_mistype("c", true) && names.may_mistype("c", false));
    // a member the compiler mints keeps its type
    assert!(!names.may_mistype("__state", false));
}

#[test]
fn a_receiver_that_never_holds_a_struct_cell_shadows_nothing() {
    let p = crate::check::Type::struct_of(vec![(PropKey::from("y"), Type::Number)]);
    let any_of = |names: &[&str]| {
        let p = p.clone();
        let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        move |ast: &Ast| {
            let mut t: HashMap<ExprId, Type> = HashMap::new();
            for n in &names {
                t.extend(ident_types(ast, n, Type::Any));
            }
            t.extend(ident_types(ast, "p", p.clone()));
            t
        }
    };
    let n = settled(
        "const cfg: any = {}; cfg.x = v; cfg[\"w\"] = v; Reflect.set(cfg, \"u\", v);",
        any_of(&["cfg", "v"]),
    );
    assert!(!n.may_shadow("x", false) && !n.may_shadow("w", false));
    assert!(!n.may_shadow("u", false));
    // a JSON value, a literal without the key, a ternary of both
    let n = settled(
        "let j: any = JSON.parse(s); j.x = v; const t: any = c ? {} : { z: 1 }; t.x = v;",
        any_of(&["j", "t", "v"]),
    );
    assert!(!n.may_shadow("x", false));
    // a computed key on an empty literal reaches no slot; on a
    // literal with keys it may reach one
    let n = settled(
        "const counts: any = {}; counts[w] = v;",
        any_of(&["counts", "v", "w"]),
    );
    assert!(!n.may_shadow("a", false));
    let n = settled(
        "const d: any = { a: 1 }; d[k] = v;",
        any_of(&["d", "v", "k"]),
    );
    assert!(n.may_shadow("a", false) && n.may_shadow("b", false));
    // a literal with the key, an assigned struct, a parameter, a reviver
    for src in [
        "const o: any = { x: 1 }; o.x = v;",
        "let o: any = {}; o = p; o.x = v;",
        "function f(o: any) { o.x = v; }",
        "const o: any = JSON.parse(s, r); o.x = v;",
    ] {
        let n = settled(src, any_of(&["o", "v"]));
        assert!(n.may_shadow("x", false), "{src}");
    }
}
