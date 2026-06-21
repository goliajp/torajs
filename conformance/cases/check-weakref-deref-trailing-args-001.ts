// ES §26.1.3.2 — WeakRef.prototype.deref(...trailing) trailing-arg ignore.
// spec is 0-arg; tora's static-table sig `(WeakRef, "deref") -> Function([],
// Nullable<Any>)` at check.rs ~3661 rejected 1+ arg calls with "expected
// 0 argument(s), got N". S325 widens via per-method carve-out in check.rs
// (typecheck-and-drop args[..]) + ssa_lower peeks recv via expr_types so
// the WeakRef-only gate stays narrow and trailing args lower-and-drop so
// step()-style side-effect exprs fire per ES eval-then-discard.

function step(label: string): number {
  console.log(label);
  return 0;
}

const obj = { x: 1 };
const wr = new WeakRef(obj);

const a = wr.deref(step("t1") as any);
console.log("a=", typeof a);

const b = wr.deref(step("t2") as any, step("t3") as any);
console.log("b=", typeof b);

const c = wr.deref();
console.log("c=", typeof c);
