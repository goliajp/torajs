use super::inline_scalar_aliases;
use crate::ast::Stmt;

fn let_anns(src: &str) -> Vec<(String, String)> {
    let toks = crate::lexer::tokenize(src).expect("lex");
    let mut ast = crate::parser::parse(src, &toks).expect("parse");
    inline_scalar_aliases(&mut ast);
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::LetDecl {
                name,
                type_ann: Some(ann),
                ..
            } => Some((name.clone(), ann.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_recursive_function_type_alias_keeps_its_name() {
    let anns = let_anns(
        "type F = (f: F, n: number) => number;\n\
         type A = (b: B) => number;\n\
         type B = (a: A) => number;\n\
         type C = (a: A) => number;\n\
         type G = (x: number) => number;\n\
         let f: F = null as any;\n\
         let a: A = null as any;\n\
         let c: C = null as any;\n\
         let g: G = null as any;\n",
    );
    let ann = |n: &str| anns.iter().find(|(b, _)| b == n).unwrap().1.clone();
    assert_eq!(ann("f"), "F");
    assert_eq!(ann("a"), "A");
    assert_eq!(ann("c"), "C");
    assert_eq!(ann("g"), "__fn(number)->(number)");
}
