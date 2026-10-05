// A generator reached through `any`, or called with an `any` argument,
// binds each argument as passed: a slot declared `string` / struct /
// array that receives another kind of value reads it as that value.

type P = { a: number };

function* lens(s: string) {
  yield s.length;
  yield typeof s;
}

const anyLens: any = lens;
for (const v of [5, null, undefined, { length: 9 }, "abc"]) {
  const it = anyLens(v);
  if (v === null || v === undefined) {
    try {
      it.next();
    } catch (e) {
      console.log("threw", e instanceof TypeError);
    }
  } else {
    console.log(it.next().value, it.next().value, it.next().done);
  }
}

// a generator expression with no captures
const expr: any = function* (s: string) {
  yield s.length;
};
console.log(expr(5).next().value, expr("abcd").next().value);

// several parameters, struct and array slots
function* multi(p: P, xs: number[], tag: string) {
  yield p.a;
  yield xs.length;
  yield tag + "!";
}
const anyMulti: any = multi;
console.log([...anyMulti({ a: 1 }, [1, 2], "t")].join(","));
console.log([...anyMulti(3, "xyz", 4)].join(","));
console.log([...anyMulti({ a: 2, b: 5 }, { length: 7 }, null)].join(","));

// direct call with an `any` argument
const n: any = 42;
const s: any = "hello";
console.log(lens(n).next().value, lens(s).next().value);

// captures of an enclosing local
function makeCounter(prefix: string) {
  const step = 10;
  const g: any = function* (s: string) {
    yield prefix + s.length;
    yield step;
  };
  return g;
}
const counter = makeCounter("n=");
console.log([...counter(7)].join(" "), [...counter("ab")].join(" "));

let shared = 1;
const byRef: any = function* (s: string) {
  yield s.length;
  shared = shared + 1;
  yield shared;
};
console.log([...byRef(5)].join(" "), [...byRef("xy")].join(" "), shared);

// a local the generator writes, read by reference through its wrapper
function bump() {
  let k = 0;
  const g: any = function* (s: string) {
    k = k + 1;
    yield s.length;
    yield k;
  };
  console.log([...g("abc")].join(" "), [...g(5)].join(" "));
  return k;
}
console.log("bump", bump());

// for..of over the generator object
let total = 0;
for (const v of anyLens({ length: 3 })) {
  console.log("for-of", v);
  if (typeof v === "number") total += v;
}
console.log("total", total);

// return() and throw()
function* guarded(s: string) {
  try {
    yield s.length;
    yield 2;
  } finally {
    console.log("finally", typeof s);
  }
}
const anyGuarded: any = guarded;
const r = anyGuarded(8);
console.log(r.next().value);
console.log(JSON.stringify(r.return(99)));
console.log(JSON.stringify(r.next()));
const t = anyGuarded("ok");
console.log(t.next().value);
try {
  t.throw(new Error("boom"));
} catch (e: any) {
  console.log("caught", e.message);
}
console.log(JSON.stringify(t.next()));

// async generators take the same route
async function* agen(s: string) {
  yield s.length;
  yield typeof s;
}
const anyAgen: any = agen;
async function drain() {
  for await (const v of anyAgen(5)) console.log("async", v);
  for await (const v of anyAgen("abc")) console.log("async", v);
  const it = anyAgen({ length: 4 });
  console.log("async next", (await it.next()).value);
  console.log("async return", JSON.stringify(await it.return(1)));
}
drain();
