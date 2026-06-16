// S130 narrow: class-field WeakMap<K,V> / WeakSet<T> member-call shape.
// Mirror of check-class-field-map-set-001 (commit eab88d7c) for the
// WeakMap / WeakSet dispatcher. `w.m.set(k, v)` where `m: WeakMap<K,V>`
// is a class field: receiver `w.m` is Expr::Member with checked type
// check::Type::WeakMap. Pre-fix the recv_ty_hint match only handled
// Expr::Ident receivers so dispatch fell through and panicked
// "unsupported member call shape: set" / "add" / "has" / "delete".
//
// Acceptance covers the 4 dispatcher arms (set / add / has / delete);
// `get` value-face is an orthogonal wedge — weakmap_get returns
// Type::Ptr and console multi-arg coercion of Ptr is unsupported, so
// the value end is observed indirectly via `has` toggling. V uses Str
// (heap, naturally Ptr-shaped) to match weakmap-001-basic and avoid
// number-as-ptr boxing which is independent of the dispatcher fix.
class K {
  x: number = 1;
  constructor(x: number) {
    this.x = x;
  }
}
class W {
  m: WeakMap<K, string>;
  s: WeakSet<K>;
  constructor() {
    this.m = new WeakMap<K, string>();
    this.s = new WeakSet<K>();
  }
}
const w = new W();
const k1 = new K(7);
const k2 = new K(11);
w.m.set(k1, "alpha");
w.s.add(k2);
console.log(w.m.has(k1), w.m.has(k2));
console.log(w.s.has(k1), w.s.has(k2));
w.m.delete(k1);
w.s.delete(k2);
console.log(w.m.has(k1), w.s.has(k2));
