// Two interfaces whose methods return an any value, one nested in the
// other as an object type literal member. A literal of the outer shape
// is respelled with the outer interface, and the literal in its member
// with the inner one, so both still take the class instances and call
// the methods, which hand the values back as they are.
const raw: any = JSON.parse('{"x":1.5,"y":"s"}');

interface I {
  m(): number;
}
interface K {
  i: { m(): number };
  k(): number;
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
class T implements K {
  i: { m(): number };
  v: any;
  constructor(v: any) {
    this.i = new P(v);
    this.v = v;
  }
  k(): number {
    return this.v.y;
  }
}
function f(o: { i: { m(): number }; k(): number }) {
  return o.i.m() + o.k();
}
function g(o: { m(): number }) {
  return o.m();
}
const t = new T(raw);
console.log(f(t), g(t.i), g(new P(raw)));
function deep(w: { o: { i: { m(): number }; k(): number } }) {
  return w.o.i.m();
}
console.log(deep({ o: t }));
console.log(f({ i: { m: () => 2 }, k: () => 3 }), deep({ o: { i: new P(raw), k: () => 4 } }));
