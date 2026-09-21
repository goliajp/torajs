// §10.4.1.2 BoundFunctionCreate copies [[Construct]] iff the target
// has one, so `new (f.bind(null))()` asks IsConstructor of the
// TARGET. tr asked it at the CELL level — "is this cell shaped like
// a constructor?" — and the Proxy row (§10.5.14 step 3: a Proxy has
// [[Construct]] exactly when its target does) only ever lived at the
// VALUE level. So `new (proxyOfClass.bind(null))()` refused, while
// `new proxyOfClass()` and `new plainClass.bind(null)()` both worked:
// the two halves each answered correctly on their own and the
// composition fell between them.
//
// This is the construct-direction twin of the call-direction fix in
// bind-proxy-target-001 — same shape, other verb.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
class K {
  x: any;
  constructor() {
    this.x = 7;
  }
}
const pk: any = new Proxy(K as any, {});

// The composition that was refused.
console.log("new bound proxy class ", attempt(() => new (pk.bind(null))().x));
// Every link of the chain, not just the first — the recursion point
// itself had to move up to the value level.
console.log("new bound bound proxy ", attempt(() => new (pk.bind(null).bind(null))().x));
// Controls: each half alone already worked, and still does.
console.log("new proxy direct      ", attempt(() => new pk().x));
console.log("new bound plain class ", attempt(() => new ((K as any).bind(null))().x));

// §10.5.14 step 3 is about the TARGET, not the handler — a handler
// without a `construct` trap changes nothing either way.
const pk2: any = new Proxy(K as any, { get: (t: any, k: any) => (t as any)[k] });
console.log("handler w/o ctor trap ", attempt(() => new (pk2.bind(null))().x));

// A proxy over something with no [[Construct]] must stay refused
// through bind — the point is to forward the target's answer, not to
// answer yes.
const arrow: any = () => 1;
const pa: any = new Proxy(arrow, {});
console.log("new bound proxy arrow ", attempt(() => new (pa.bind(null))()));
console.log("new proxy arrow direct", attempt(() => new pa()));
const pm: any = new Proxy(Array.prototype.join as any, {});
console.log("new bound proxy method", attempt(() => new (pm.bind(null))()));

// A bound builtin is unmoved (kind-0 targets never reach the Proxy
// row, and must not start answering yes).
console.log("new bound Array       ", attempt(() => (new ((Array as any).bind(null))(3) as any).length));
console.log("new bound join        ", attempt(() => new ((Array.prototype.join as any).bind([1]))()));
// Partial arguments ride along through the proxy.
class P2 {
  s: any;
  constructor(a: any, b: any) {
    this.s = a + "/" + b;
  }
}
const pp: any = new Proxy(P2 as any, {});
console.log("bound partial args    ", attempt(() => new (pp.bind(null, "a"))("b").s));
