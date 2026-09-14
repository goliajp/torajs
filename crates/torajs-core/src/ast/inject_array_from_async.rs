//! `Array.fromAsync` self-hosted (RFC 20260914-array-from-async-ts).
//!
//! proposal-array-from-async §2.1.1 is an async abstract closure:
//! it awaits between every step (each `next()` result, each element
//! off a sync source, each mapped value). A kernel that walks the
//! source first and settles afterwards cannot spell that — a
//! thenable element, a source mutated after the call, the
//! observable get order all came out wrong. The body is TypeScript
//! now: an `async function` this pass injects, which the ordinary
//! async desugar turns into a machine like any user async function.
//!
//! Two routes reach the one body. The direct spellings
//! (`Array.fromAsync(…)`, `.call(C, …)`, `.apply(C, [...])`) are
//! rewritten here to a call of the helper by name — `__c` first, the
//! rest padded to the helper's arity with `undefined`. The runtime
//! ns-static cell (`(Array as any).fromAsync(…)`, a reified
//! `Array.fromAsync` value) forwards to the closure the injected
//! `__torajs_ns_static_bind` statement registers at startup.
//!
//! Gated on the member name `fromAsync` appearing anywhere (the
//! `inject_disposable_stack` over-approximation): a program that
//! never mentions it pays nothing.

use super::{Ast, Expr, ExprId, Stmt};

/// The helper's name — a declaration the parser binds like any other.
pub(crate) const HELPER: &str = "__array_from_async";

/// Every step of proposal-array-from-async §2.1.1, in spec order.
/// `IsConstructor(C)` is `typeof c === "function"` here (a callable
/// non-constructor |this| is the one recorded approximation).
const HELPER_SRC: &str = r#"
async function __array_from_async_close(__iter: any): Promise<void> {
  let __ret: any;
  try {
    __ret = __iter["return"];
    if (__ret === undefined || __ret === null) {
      return;
    }
    const __r: any = __ret.call(__iter);
    await __r;
  } catch (__e: any) {
  }
}

async function __array_from_async(__c: any, __items: any, __mapfn: any, __this_arg: any): Promise<any[]> {
  const __mapping: boolean = __mapfn !== undefined;
  if (__mapping && typeof __mapfn !== "function") {
    throw new TypeError("Array.fromAsync: mapfn is not callable");
  }
  let __using_async: any = __items[Symbol.asyncIterator];
  let __using_sync: any = undefined;
  if (__using_async === undefined || __using_async === null) {
    __using_async = undefined;
    __using_sync = __items[Symbol.iterator];
    if (__using_sync === null) {
      __using_sync = undefined;
    }
    if (__using_sync !== undefined && typeof __using_sync !== "function") {
      throw new TypeError("Array.fromAsync: Symbol.iterator is not callable");
    }
  } else if (typeof __using_async !== "function") {
    throw new TypeError("Array.fromAsync: Symbol.asyncIterator is not callable");
  }
  if (__using_async !== undefined || __using_sync !== undefined) {
    const __is_async: boolean = __using_async !== undefined;
    const __iter: any = __is_async ? __using_async.call(__items) : __using_sync.call(__items);
    if (__iter === null || (typeof __iter !== "object" && typeof __iter !== "function")) {
      throw new TypeError("Array.fromAsync: iterator is not an object");
    }
    const __next: any = __iter.next;
    const __a: any = typeof __c === "function" ? new __c() : [];
    let __k: number = 0;
    while (true) {
      let __r: any = __next.call(__iter);
      if (__is_async) {
        __r = await __r;
      }
      if (__r === null || (typeof __r !== "object" && typeof __r !== "function")) {
        throw new TypeError("Array.fromAsync: iterator result is not an object");
      }
      const __done: boolean = __r.done ? true : false;
      if (__done) {
        if (!Reflect.set(__a, "length", __k)) {
          throw new TypeError("Array.fromAsync: cannot set length");
        }
        return __a;
      }
      let __v: any = __r.value;
      if (!__is_async) {
        try {
          __v = await __v;
        } catch (__e: any) {
          await __array_from_async_close(__iter);
          throw __e;
        }
      }
      let __mv: any = __v;
      if (__mapping) {
        try {
          __mv = __mapfn.call(__this_arg, __v, __k);
          __mv = await __mv;
        } catch (__e: any) {
          await __array_from_async_close(__iter);
          throw __e;
        }
      }
      try {
        Object.defineProperty(__a, __k, { value: __mv, writable: true, enumerable: true, configurable: true });
      } catch (__e: any) {
        await __array_from_async_close(__iter);
        throw __e;
      }
      __k = __k + 1;
    }
  }
  const __obj: any = Object(__items);
  let __len: any = +__obj.length;
  if (__len !== __len || __len <= 0) {
    __len = 0;
  }
  if (__len > 9007199254740991) {
    __len = 9007199254740991;
  }
  __len = Math.trunc(__len);
  if (typeof __c !== "function" && __len > 4294967295) {
    throw new RangeError("Invalid array length");
  }
  const __al: any = typeof __c === "function" ? new __c(__len) : [];
  let __i: number = 0;
  while (__i < __len) {
    let __v: any = __obj[__i];
    __v = await __v;
    let __mv: any = __v;
    if (__mapping) {
      __mv = __mapfn.call(__this_arg, __v, __i);
      __mv = await __mv;
    }
    Object.defineProperty(__al, __i, { value: __mv, writable: true, enumerable: true, configurable: true });
    __i = __i + 1;
  }
  if (!Reflect.set(__al, "length", __len)) {
    throw new TypeError("Array.fromAsync: cannot set length");
  }
  return __al;
}
"#;

/// The helper's parameter count — every rewritten call is padded to it.
const ARITY: usize = 4;

fn referenced(ast: &Ast) -> bool {
    ast.exprs
        .iter()
        .any(|e| matches!(e, Expr::Member { name, .. } if name == "fromAsync"))
}

/// `Array.fromAsync` spelled as a static read — the callee of the
/// direct form, the object of `.call` / `.apply`.
fn is_static_read(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Member { obj, name }
        if name == "fromAsync" && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Array"))
}

/// The helper's argument list for a call the pass rewrites, `__c`
/// first. `None` when the callee is not one of the three spellings,
/// or when a literal argArray carries a spread (no static length).
/// A written argument past the helper's arity stays with the cell
/// route (nothing in the wild writes one).
fn helper_args(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Option<Vec<Option<ExprId>>> {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return None;
    };
    let mut out: Vec<Option<ExprId>> = Vec::new();
    if name == "fromAsync" && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Array") {
        out.push(None);
        out.extend(args.iter().map(|&a| Some(a)));
    } else if name == "call" && is_static_read(ast, *obj) {
        out.extend(args.iter().map(|&a| Some(a)));
    } else if name == "apply" && is_static_read(ast, *obj) {
        match args {
            [] => {}
            [this_arg] => out.push(Some(*this_arg)),
            [this_arg, arr] => {
                let Expr::Array(els) = ast.get_expr(*arr) else {
                    return None;
                };
                if els
                    .iter()
                    .any(|&e| matches!(ast.get_expr(e), Expr::Spread { .. }))
                {
                    return None;
                }
                out.push(Some(*this_arg));
                out.extend(els.iter().map(|&a| Some(a)));
            }
            _ => return None,
        }
    } else {
        return None;
    }
    if out.len() > ARITY {
        return None;
    }
    Some(out)
}

/// Rewrite every direct spelling in place (the Call keeps its
/// ExprId, so the side tables keyed on it stay valid).
fn rewrite_direct_calls(ast: &mut Ast) {
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Some(shape) = helper_args(ast, callee, &args) else {
            continue;
        };
        let mut new_args: Vec<ExprId> = Vec::with_capacity(ARITY);
        for slot in shape {
            new_args.push(match slot {
                Some(e) => e,
                None => ast.add_expr(Expr::Ident("undefined".into())),
            });
        }
        while new_args.len() < ARITY {
            new_args.push(ast.add_expr(Expr::Ident("undefined".into())));
        }
        let new_callee = ast.add_expr(Expr::Ident(HELPER.into()));
        ast.exprs[i] = Expr::Call {
            callee: new_callee,
            args: new_args,
        };
    }
}

pub fn inject_array_from_async(ast: &mut Ast) {
    if !referenced(ast) {
        return;
    }
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == HELPER))
    {
        return;
    }
    rewrite_direct_calls(ast);
    // The registration: the cell's id off the shared table, and an
    // arrow over the helper (an async declaration read as a value is
    // not an any-slot shape today; an arrow is).
    let id = torajs_rc::ns_static::ns_static_id("Array", "fromAsync");
    let src = format!(
        "{HELPER_SRC}\n__torajs_ns_static_bind({id}, (__c: any, __items: any, __mapfn: any, __this_arg: any) => {HELPER}(__c, __items, __mapfn, __this_arg));\n"
    );
    let tokens = crate::lexer::tokenize(&src).expect("array-from-async helper lex");
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let offset =
        crate::parser::parse_into(&src, &tokens, ast).expect("array-from-async helper parse");
    // The parsed spans index the helper source, not the program's —
    // every consumer slices `ast.source` with them (the
    // `inject_driver` sentinel, on all three tables).
    let sentinel = crate::lexer::Span { start: 0, end: 0 };
    for sp in ast.type_ann_spans.iter_mut().skip(type_ann_base) {
        *sp = sentinel;
    }
    for sp in ast.expr_spans.iter_mut().skip(expr_base) {
        *sp = sentinel;
    }
    for s in &mut ast.stmts[offset..] {
        crate::modules::splice::clear_injected_spans(s);
    }
    // Front-splice: the bind statement runs before any user
    // statement, so a reified cell called from the program's first
    // line already forwards.
    let injected: Vec<Stmt> = ast.stmts.split_off(offset);
    ast.stmts.splice(0..0, injected);
}
