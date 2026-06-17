// S143 — class method return-type inference (no explicit annotation).
// Mirrors top-level FnDecl inference: when a class method body has a
// value-bearing return, the synthesized `__cm_<C>__<m>(...)` FnDecl's
// return type is sniffed from the return expression. Pre-fix tr
// rejected `foo() { return 1; }` with "function expects Void, got
// Number" because the desugar_implicit_generics pass skipped the
// `__this`-prefixed shape on the (overoptimistic) assumption that
// class methods always carry a declared return annotation.
//
// Body shapes supported here use literal returns + simple operand
// shapes (Ident / BinOp on literals). Member access on `this`
// (`return this._x` from a getter) still needs the Member arm in
// infer_expr_ann_with — handoff L3b follow-up.

class A {
  foo() { return 1; }
  bar() { return "hi"; }
  baz() { return true; }
  noret() { console.log("noret"); }
  arith(n: number) { return n * 2 + 1; }
  cat(s: string) { return s + "!"; }
}
const a = new A();
console.log(a.foo());
console.log(a.bar());
console.log(a.baz());
a.noret();
console.log(a.arith(5));
console.log(a.cat("abc"));

// explicit return-type annotation still works
class B {
  ret(): number { return 42; }
}
console.log(new B().ret());
