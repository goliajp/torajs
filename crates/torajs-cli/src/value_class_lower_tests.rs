//! Whole programs whose `Token` the value-class census admits, lowered
//! to the end. Pass 2.5 reads the finished SSA of each against the
//! census and fails the lowering on any disagreement, so each of these
//! lowering at all is the SSA's agreement with the census.

use torajs_core::{check, ssa_lower};

use crate::value_class_census_tests::{BASE, BOXED, classes, desugared};

fn admitted_and_lowered(src: &str) {
    let ast = desugared(src);
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    assert_eq!(
        classes(src),
        vec!["Token".to_string()],
        "{src}\n{:?}",
        ssa_lower::value_class_verdicts(&artifacts)
    );
    let m = ssa_lower::lower_with_arity(&ast, &artifacts);
    // the check reads live function bodies; a program with none would
    // pass it unread
    assert!(
        m.funcs
            .iter()
            .any(|f| f.name == "__new_Token" && !f.is_declaration()),
        "no factory body was lowered"
    );
}

#[test]
fn the_tokenizer_lowers_in_agreement() {
    admitted_and_lowered(include_str!(
        "../../../bench/cases/tokenizer-parser-eval-10k/main.ts"
    ));
}

#[test]
fn the_base_program_lowers_in_agreement() {
    admitted_and_lowered(&BASE.replace("TAIL", ""));
}

#[test]
fn a_view_off_a_container_field_lowers_in_agreement() {
    admitted_and_lowered(&BOXED.replace(
        "BODY",
        "const h = new Holder(make(3))\nconst t = h.toks[1]\ns = s + t.kind + h.toks[2].value\n",
    ));
}

#[test]
fn every_push_spelling_lowers_in_agreement() {
    // a fill loop the lowering reserves ahead of, and a container field
    admitted_and_lowered(&BASE.replace(
        "TAIL",
        "const more: Token[] = []\nfor (let i = 0; i < 4; i = i + 1) more.push(new Token(i, 1))\ns = s + more[3].kind\n",
    ));
    admitted_and_lowered(&BOXED.replace(
        "BODY",
        "const h = new Holder(make(2))\nh.toks.push(new Token(3, 4))\ns = s + h.toks[2].kind\n",
    ));
}

#[test]
fn a_view_shadowed_in_a_block_lowers_in_agreement() {
    let tail = "const t = ts[0]\n{\n  const t = ts[1]\n  s = s + t.kind\n}\ns = s + t.kind\n";
    admitted_and_lowered(&BASE.replace("TAIL", tail));
}
