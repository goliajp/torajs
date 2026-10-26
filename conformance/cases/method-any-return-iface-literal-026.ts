// A method of a class whose interface member returns an any value, read
// as a value only to inspect it (typeof, its name, identity, a Set key)
// off a receiver typed as the class and as the interface: the hierarchy
// still returns the any value. A user method named `get` handed a
// function calls it, so it is no inspection.
const raw: any = JSON.parse('{"x":1.5}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class Reg {
  get(f: () => number): number {
    return f() + 1;
  }
}
function call(o: J) {
  return o.m();
}
const s = new S(raw);
const j: J = s;
const set = new Set<Function>();
set.add(j.m);
console.log(typeof s.m, s.m.name, j.m.name, typeof j.m, set.has(j.m), s.m === s.m);
console.log(call(s), s.m(), j.m(), new Reg().get(() => 2));
