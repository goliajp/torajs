// An object literal may leave out an optional field of the type it is
// checked against, at every position a literal meets a declared type.
// The field reads as undefined and is not an own property.

type P = { a?: number; b: number };

const arrow = (p: { a?: number; b: number }) => p.b;
console.log(arrow({ b: 2 }));

function decl(p: P): string {
  return `${p.a === undefined} ${p.a ?? 5} ${"a" in p} ${p.b}`;
}
console.log(decl({ b: 3 }));
console.log(decl({ a: 1, b: 4 }));

class K {
  m(p: { a?: string; b: number }): string {
    return (p.a ?? "none") + ":" + p.b;
  }
}
console.log(new K().m({ b: 1 }), new K().m({ a: "x", b: 2 }));

const v: { a?: number; b: number } = { b: 2 };
console.log(v.a, v.b, "a" in v, Object.keys(v).join(","));
v.a = 3;
console.log(v.a, v.a + 1, "a" in v, Object.keys(v).length);

function ret(): P {
  return { b: 7 };
}
const r = ret();
console.log(r.a, r.b, "a" in r);

const xs: P[] = [{ b: 1 }, { a: 2, b: 3 }];
console.log(xs.map((x) => `${x.a}/${x.b}/${"a" in x}`).join(" "));

type Outer = { inner: { s?: string; n: number }; tag?: string };
const o: Outer = { inner: { n: 1 } };
console.log(o.inner.s, o.inner.n, "s" in o.inner, o.tag, "tag" in o);

type Many = { a?: number; s?: string; f?: () => number; xs?: number[]; b: boolean };
const m: Many = { b: true };
console.log(m.a, m.s, m.f, m.xs, m.b, Object.keys(m).length);
console.log(JSON.stringify(m), Object.hasOwn(m, "s"), m.hasOwnProperty("xs"));
m.f = () => 9;
m.xs = [1];
console.log(m.f(), m.xs.length, Object.keys(m).length);

for (const k in { b: 1 } as P) console.log("key", k);
console.log(r, m.b);
