// Interfaces whose methods return an any value, one held in the other
// inside a dictionary's and a promise's value type. A literal of the
// outer shape is matched with the outer interface whichever literal it
// holds in the dictionary member, and the shapes nested in the
// dictionary and the promise follow the inner interface's retype, so
// the class instances and the literals both answer the values stored.
const raw: any = JSON.parse('{"x":1.5,"y":"s"}');
interface I {
  m(): number;
}
class P implements I {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
interface K {
  d: Record<string, { m(): number }>;
  k(): number;
}
class T implements K {
  d: Record<string, { m(): number }>;
  v: any;
  constructor(v: any) {
    this.d = { a: new P(v) };
    this.v = v;
  }
  k(): number {
    return this.v.y;
  }
}
function f(o: { d: Record<string, { m(): number }>; k(): number }) {
  return o.d["a"].m() + o.k();
}
function g(o: { m(): number }) {
  return o.m();
}
const t = new T(raw);
console.log(f(t), g(t.d["a"]), g(new P(raw)));
console.log(f({ d: { a: { m: () => 2 } }, k: () => 3 }));
interface K3 {
  p: Promise<{ m(): number }>;
  k(): number;
}
class T3 implements K3 {
  p: Promise<{ m(): number }>;
  v: any;
  constructor(v: any) {
    this.p = Promise.resolve(new P(v));
    this.v = v;
  }
  k(): number {
    return this.v.y;
  }
}
async function f3(o: { p: Promise<{ m(): number }>; k(): number }) {
  const q = await o.p;
  return q.m() + o.k();
}
f3(new T3(raw)).then((v) => console.log(v));
f3({ p: Promise.resolve({ m: () => 2 }), k: () => 3 }).then((v) => console.log(v));
