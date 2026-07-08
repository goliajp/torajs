// chunk 711 — builtin method reification: reading a method name off
// an any builtin receiver yields the unbound function object.
const s: any = "hello";
const f = s.toUpperCase;
console.log(typeof f);
console.log(f === s.toUpperCase);
console.log(f.call(s));
console.log(f.call("other"));
try {
  f();
} catch (e) {
  console.log("bare threw:", e instanceof TypeError);
}

const g = s.slice;
console.log(g.call(s, 1, 3));
console.log(g.apply(s, [1, 3]));

const n: any = 42.5;
const tf = n.toFixed;
console.log(typeof tf);
console.log(tf.call(n, 1));

// wrong-arm reads stay undefined (exact per-receiver table)
console.log((42 as any).slice);
console.log(typeof (true as any).toString);

// map methods re-bind through .call
const m: any = new Map();
const ms = m.set;
ms.call(m, "k", 7);
console.log(m.get("k"));

// date getter through .apply
const d: any = new Date(0);
const gt = d.getTime;
console.log(gt.apply(d));

// a closure's own call/apply reify too
const h: any = (x: number) => x + 1;
console.log(typeof h.call);
const hc = h.call;
console.log(hc.call(h, null, 7));

// reified method as an HOF callback runs with this = undefined
try {
  (["a"] as any).map(s.toUpperCase);
  console.log("hof no throw");
} catch (e) {
  console.log("hof threw:", e instanceof TypeError);
}
