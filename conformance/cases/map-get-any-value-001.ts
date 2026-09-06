// `m.get(k)` on a `Map<string, any>` answered `Nullable(Any)` — a
// second name for one type. With the value type erased, TS's
// `V | undefined` IS `any`, and every use of the result that was not
// a bare read got refused: calling it ("not callable: type
// Nullable(Any)"), adding to it, `??`-ing it, reading a member of a
// `WeakRef.deref()`. The undefined-ness the extra name was recording
// is carried by the value itself — it is a tagged Any either way —
// so nothing below changed except that the checker stopped refusing.

function p(a: any): string {
  return "p" + a;
}

const m = new Map<string, any>();
m.set("fn", p);
m.set("num", 7);
m.set("str", "x");

// the uses that were refused
console.log(m.get("fn")(1));
console.log(m.get("num") + 1);
console.log(m.get("str") + "!");
console.log(m.get("missing") ?? 5);
const held = m.get("fn");
console.log(held(2));

// the undefined-observing uses, unchanged
console.log(m.get("missing"));
console.log(m.get("missing") === undefined, m.get("num") === 7);
console.log(typeof m.get("missing"), typeof m.get("num"));
console.log(m.get("missing") ? "y" : "n", m.get("num") ? "y" : "n");

// calling a missing key is still a catchable TypeError, not a
// compile-time refusal
try {
  m.get("missing")(1);
  console.log("no throw");
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}

// the same spelling in the weak family
const key: any = {};
const w = new WeakMap<any, any>();
w.set(key, p);
console.log(w.get(key)(3));
const wn = new WeakMap<any, any>();
wn.set(key, 7);
console.log(wn.get(key) + 1, wn.get({}) === undefined);

const target: any = { v: 3 };
const r = new WeakRef(target);
console.log(r.deref().v);
