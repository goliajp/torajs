// §28.1.13 step 4 — Reflect.set(target, key, V, receiver) is
// `target.[[Set]](key, V, receiver)`: the property lookup walks the
// TARGET, and the write (plus any setter's `this`) goes to the
// RECEIVER. Every other [[Set]] in the language hands both roles to
// one object, which is why this is the only spelling that tells them
// apart.
function show(label: string, v: any) {
  console.log(label, JSON.stringify(v));
}

// A writable data property on the target: the write lands on the
// receiver as a fresh own property, and the target keeps its value.
const t1: any = { y: 1 };
const r1: any = {};
show("data", [
  Reflect.set(t1, "y", 8, r1),
  t1.y,
  r1.y,
  Object.prototype.hasOwnProperty.call(r1, "y"),
]);

// An accessor on the target runs with the receiver as `this`, so
// everything it writes lands there.
const t2: any = {
  set p(v: any) {
    this._p = v;
  },
};
const r2: any = {};
show("accessor", [Reflect.set(t2, "p", 5, r2), r2._p, t2._p]);

// The lookup is a full chain walk from the target — an inherited
// setter counts, and still runs against the receiver.
const gp: any = {
  set q(v: any) {
    this._q = v;
  },
};
const t3: any = Object.create(gp);
const r3: any = {};
show("inherited-accessor", [Reflect.set(t3, "q", 7, r3), r3._q, t3._q, gp._q]);

// Step 2.a — a non-writable data property on the target refuses,
// whatever the receiver looks like.
const t4: any = {};
Object.defineProperty(t4, "n", { value: 1, writable: false });
const r4: any = {};
show("target-nonwritable", [Reflect.set(t4, "n", 5, r4), r4.n, t4.n]);

// Steps 2.d.i-ii — the receiver's OWN entry gets a veto: an accessor
// there, or a non-writable data property, refuses the write.
const t5: any = { k: 1 };
const r5: any = {};
Object.defineProperty(r5, "k", {
  get() {
    return 0;
  },
  configurable: true,
});
show("receiver-own-accessor", [Reflect.set(t5, "k", 9, r5), r5.k]);

const t6: any = { w: 1 };
const r6: any = {};
Object.defineProperty(r6, "w", { value: 0, writable: false });
show("receiver-own-nonwritable", [Reflect.set(t6, "w", 9, r6), r6.w]);

// Step 2.b — a primitive receiver has nowhere to hold the property,
// and the answer is a plain false rather than a throw.
const t7: any = { y: 1 };
show("primitive-receiver", [
  Reflect.set(t7, "y", 2, 5),
  Reflect.set(t7, "y", 3, null),
  t7.y,
]);

// Absent from the whole chain — step 2.e creates it on the receiver.
const t8: any = {};
const r8: any = {};
show("absent", [Reflect.set(t8, "z", 3, r8), r8.z, t8.z]);

// The key domain is ToPropertyKey, so a symbol takes the same route.
const sym = Symbol("k");
const t9: any = { [sym]: 1 };
const r9: any = {};
show("symbol-key", [Reflect.set(t9, sym, 4, r9), r9[sym], t9[sym]]);

// Step 3 — an omitted receiver defaults to the target, and passing
// the target explicitly is the same thing.
const t10: any = { a: 1 };
show("default-receiver", [
  Reflect.set(t10, "a", 2),
  t10.a,
  Reflect.set(t10, "a", 3, t10),
  t10.a,
  Reflect.set(t10, "fresh", 4),
  t10.fresh,
]);

// The detached call carries the same four-argument shape, and the
// function's own `length` stays at the spec's 3.
const detached: any = Reflect.set;
const t11: any = { y: 1 };
const r11: any = {};
show("detached", [detached(t11, "y", 8, r11), t11.y, r11.y]);
show("face", [Reflect.set.length, Reflect.set.name]);
