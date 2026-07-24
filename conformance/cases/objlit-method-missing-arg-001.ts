// T-28 pad on the struct-method dispatch lane — an object-literal
// method / fn-valued field called with fewer args than declared must
// see `undefined` in the missing any-slots (ES §10.2.1.4), not a
// stray register value. Pre-fix the missing `x: any` answered a
// Str-tagged garbage value: `typeof x` said "string", `String(x)`
// answered "", and `x == null` said false while console.log printed
// blank — the two faces of the same unpadded CallIndirect.

// face 1 — method shorthand, print + loose-eq must agree
const b = { m(x: any) { console.log(x); return x == null; } };
console.log(b.m());
console.log(b.m(undefined));
console.log(b.m(null));

// face 2 — typeof across the call shapes that share the lane
const c = { m: (x: any) => { console.log(typeof x); } };
c.m();
const d = { m(x: any) { console.log(typeof x); } };
d.m();

// face 3 — a same-named class method routes the objlit call through
// the speculative `__cm_` rewrite + demote; the pad recorded on the
// demoted alt node must travel back to the restored call site
const e = { g(x: any) { console.log(typeof x); console.log(x === undefined); } };
e.g();
class K { g(x: any) { console.log("class"); } }
new K().g();

// face 4 — two missing slots pad independently
const f = { m(x: any, y: any) { console.log(typeof x, typeof y); } };
f.m();
f.m(1);
