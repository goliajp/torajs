// RFC 20260730-iterator-global 刀 4 — Iterator.prototype.flatMap
// (§27.1.4.5): lazy, array/generator/iterator-valued mappers
// flatten through GetIteratorFlattenable (REJECT-PRIMITIVES — a
// string mapped value refuses), empty inners skip, (value, counter)
// args, take chaining short-circuit, for-of consumption.

function* g() {
  yield 1;
  yield 2;
}

// Array-valued mapper flattens in order.
const a: any = [1, 2].values();
console.log(JSON.stringify(a.flatMap((v: any) => [v, v * 10]).toArray()));

// Generator-valued mapper.
function* pair(x: any) {
  yield x;
  yield -x;
}
const b: any = g();
console.log(JSON.stringify(b.flatMap((v: any) => pair(v)).toArray()));

// Lazy: mapper does not run until stepped.
let calls = 0;
const c: any = g();
const fm: any = c.flatMap((v: any) => {
  calls++;
  return [v];
});
console.log(calls);
console.log(JSON.stringify(fm.next()));
console.log(calls);

// Empty inners skip.
const d: any = [1, 2, 3].values();
console.log(
  JSON.stringify(d.flatMap((v: any) => (v === 2 ? [] : [v])).toArray()),
);

// (value, counter) callback args.
const e2: any = ["x", "y"].values();
console.log(JSON.stringify(e2.flatMap((v: any, i: any) => [i, v]).toArray()));

// take() over flatMap short-circuits mid-inner.
const f: any = g();
console.log(
  JSON.stringify(
    f
      .flatMap((v: any) => [v, v, v])
      .take(4)
      .toArray(),
  ),
);

// for-of consumption.
const h: any = g();
for (const x of h.flatMap((v: any) => [v * 100])) {
  console.log(x);
}

// A string mapped value refuses (REJECT-PRIMITIVES).
const i2: any = g();
const bad: any = i2.flatMap((v: any) => "ab");
try {
  bad.next();
} catch (e3: any) {
  console.log(e3 instanceof TypeError);
}

// flatMap over an Iterator.from wrap.
let jidx = 0;
const jvals = [3, 4];
const j: any = Iterator.from({
  next() {
    return jidx < jvals.length
      ? { value: jvals[jidx++], done: false }
      : { value: undefined, done: true };
  },
});
console.log(JSON.stringify(j.flatMap((v: any) => [v, v]).toArray()));
