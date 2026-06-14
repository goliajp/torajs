// Class field `m: Map<K,V>` / `s: Set<T>` member-call dispatch.
// Pre-fix: SSA-lower P6.1 / P6.2 Map/Set method dispatcher's
// `recv_ty_hint` only inspected `Expr::Ident` receivers (local
// bindings), so `new W().m.set(...)` — whose receiver is
// `Expr::Member` — fell through to the module.method fallback and
// panicked "unsupported member call shape: set". Now the dispatcher
// also reads `expr_types` for Member / Index / Call receivers, picking
// up the checked `Type::Map` / `Type::Set` from class-field-typed
// targets.
class W {
  m: Map<string, number>;
  s: Set<number>;
  constructor() {
    this.m = new Map();
    this.s = new Set();
  }
}
const w = new W();
w.m.set("a", 1);
w.m.set("b", 2);
w.s.add(10);
w.s.add(20);
console.log(w.m);
console.log(w.s);
console.log(w.m.size);
console.log(w.s.size);
console.log(w.m.has("a"));
console.log(w.s.has(10));
