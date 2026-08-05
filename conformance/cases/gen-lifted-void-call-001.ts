// RFC 20260805-async-fn-state-machine D0, fourth half — a generator
// local initialized by a call that produces no value.
//
// Such a call produces `undefined` (§14.10), and `check_stmt_let_decl`
// already says so — it normalizes Void to Undefined so that "every
// consumer (print / any-box)" stops panicking. But it says it only for
// a `let` init, and the generator lift rewrites `let a = f()` into
// `this.a = f()` — an assignment. Two consequences met there:
//
// A: the lift's `number` fallback. "field is Number, value is
//    Undefined" — the local would not compile at all.
// B: `box_to_any` had no `Void` arm, so once the field was typed to
//    hold the value the lowerer panicked instead
//    ("box_to_any element type Void not supported").
//
// Both spellings answer the same thing: a function that wrote `: void`
// and one that simply never returns a value.

function sideOnly() {
  console.log("side");
}
function declaredVoid(): void {
  console.log("declared");
}
function* g(): any {
  const a = sideOnly();
  yield a;
  console.log(typeof a);
  yield "mid";
  const b = declaredVoid();
  yield b;
  console.log(a === b);
  return 0;
}

function drain(gg: any): void {
  let r: any = gg.next();
  while (r.done === false) {
    console.log(r.value);
    r = gg.next(0);
  }
}

drain(g());
