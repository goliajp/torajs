//! The import-options half of §13.3.10 EvaluateImportCall (steps
//! 10-11): the dispatcher calls this helper after ToString(specifier),
//! inside the same try whose catch turns every throw into the promise
//! reject (IfAbruptRejectPromise).

use crate::ast::{Ast, Stmt};

pub(super) const OPTIONS_HELPER: &str = "__torajs_dyn_import_options";

// EnumerableOwnProperties(key+value) spelled out: [[OwnPropertyKeys]],
// then per String key one [[GetOwnProperty]] and, when enumerable, one
// [[Get]] — a proxy's traps see exactly that sequence. The host
// supports no attribute (AllImportAttributesSupported, same verdict as
// the static `with` clause), so any entry rejects once every value has
// been read and checked.
const HELPER_SRC: &str = r#"
function __torajs_dyn_import_options(__opts: any): void {
  if (__opts === undefined) return;
  if (__opts === null || (typeof __opts !== "object" && typeof __opts !== "function")) {
    throw new TypeError("the options argument of import() must be an object");
  }
  const __with: any = __opts.with;
  if (__with === undefined) return;
  if (__with === null || (typeof __with !== "object" && typeof __with !== "function")) {
    throw new TypeError("the 'with' option of import() must be an object");
  }
  const __keys: any = Reflect.ownKeys(__with);
  let __first: any = undefined;
  for (let __i: number = 0; __i < __keys.length; __i++) {
    const __k: any = __keys[__i];
    if (typeof __k !== "string") continue;
    const __d: any = Object.getOwnPropertyDescriptor(__with, __k);
    if (__d === undefined || !__d.enumerable) continue;
    const __v: any = __with[__k];
    if (typeof __v !== "string") {
      throw new TypeError("import attribute '" + __k + "' must be a string");
    }
    if (__first === undefined) __first = __k;
  }
  if (__first !== undefined) {
    throw new TypeError("import attribute '" + __first + "' is not supported");
  }
}
"#;

/// Parse the helper into `ast`'s arena and answer its FnDecl for the
/// caller to splice next to the dispatcher.
pub(super) fn synth_options_helper(ast: &mut Ast) -> Vec<Stmt> {
    let tokens = crate::lexer::tokenize(HELPER_SRC).expect("import-options helper lex");
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let offset =
        crate::parser::parse_into(HELPER_SRC, &tokens, ast).expect("import-options helper parse");
    // the parsed spans index the helper source, not the program's
    let sentinel = crate::lexer::Span { start: 0, end: 0 };
    for sp in ast.type_ann_spans.iter_mut().skip(type_ann_base) {
        *sp = sentinel;
    }
    for sp in ast.expr_spans.iter_mut().skip(expr_base) {
        *sp = sentinel;
    }
    let mut stmts = ast.stmts.split_off(offset);
    for s in &mut stmts {
        super::splice::clear_injected_spans(s);
    }
    stmts
}
