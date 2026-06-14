// L3b "class W { m: Map<K,V>; ctor: this.m = new Map() }" close (one root):
// default_init_for_type fallback used to emit Number(0) for Map/Set/WeakMap/
// WeakSet field anns → declared/init Struct mismatch fails check before SSA.
// Fix: emit `new Map()`/etc so the implicit default init matches the field
// type. Verifies type-check passes for ctor-body assign with all 4 builtins.
class CM {
  m: Map<string, number>;
  constructor() { this.m = new Map(); }
}
class CS {
  s: Set<number>;
  constructor() { this.s = new Set(); }
}
class CWM {
  wm: WeakMap<object, number>;
  constructor() { this.wm = new WeakMap(); }
}
class CWS {
  ws: WeakSet<object>;
  constructor() { this.ws = new WeakSet(); }
}
console.log(typeof new CM().m);
console.log(typeof new CS().s);
console.log(typeof new CWM().wm);
console.log(typeof new CWS().ws);
