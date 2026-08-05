// RFC 20260806-declared-field-redefine — Object.defineProperty on a
// field the class DECLARES. The value stays in the layout slot; only
// the attributes move into the instance's expando dict, so no second
// own property of the same name is created.
class C {
  x: number = 1;
  y: string = "a";
}

// value change + attribute change; the field leaves Object.keys but
// getOwnPropertyNames still carries it
const c = new C();
Object.defineProperty(c, "x", { value: 99, enumerable: false });
console.log(c.x, JSON.stringify(Object.keys(c)), JSON.stringify(Object.getOwnPropertyNames(c)));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(c, "x")));

// absent attributes keep their CURRENT value (§10.1.6.3), not false
const d = new C();
Object.defineProperty(d, "x", { value: 7 });
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(d, "x")));

// an attribute-only define leaves the value alone
const e = new C();
Object.defineProperty(e, "y", { enumerable: false });
console.log(e.y, JSON.stringify(Object.keys(e)));

// every enumeration face agrees with Object.keys
console.log(JSON.stringify(e));
const seen: string[] = [];
for (const k in e) {
  seen.push(k);
}
console.log(JSON.stringify(seen));

// writable:false is enforced by the TYPED store too, not just by
// defineProperty
const g = new C();
Object.defineProperty(g, "x", { writable: false });
try {
  g.x = 5;
  console.log("stored", g.x);
} catch (err) {
  console.log("threw", g.x);
}

// a redefined field is listed exactly once
const h = new C();
Object.defineProperty(h, "x", { value: 3 });
console.log(JSON.stringify(Object.getOwnPropertyNames(h)));

// Reflect. spelling and an `any` receiver reach the same entry
const r = new C();
console.log(Reflect.defineProperty(r, "x", { value: 42, enumerable: false }), r.x);
const q: any = new C();
Object.defineProperty(q, "x", { value: 8, enumerable: false });
console.log(q.x, JSON.stringify(Object.keys(q)));

// §10.1.6.3 validation over the field's live attributes
const n = new C();
Object.defineProperty(n, "x", { value: 1, configurable: false });
try {
  Object.defineProperty(n, "x", { value: 2, configurable: true });
  console.log("ok", n.x);
} catch (err) {
  console.log("threw", n.x);
}

// SameValue is still allowed on a locked field
const s = new C();
Object.defineProperty(s, "x", { value: 5, writable: false, configurable: false });
try {
  Object.defineProperty(s, "x", { value: 5 });
  console.log("same-ok", s.x);
} catch (err) {
  console.log("same-threw", s.x);
}
