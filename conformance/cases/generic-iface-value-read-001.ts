// A method read as a value through an interface in a generic function's
// body: the read is typed only in the specializations, and the
// interface keeps the return the value's slot spells.
interface J {
  m(): number;
}
class S implements J {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function take<T>(j: J, t: T): () => number {
  const h = j.m;
  return h;
}
const k: J = { m: () => 4 };
const s: J = new S(JSON.parse('{"x":2}'));
console.log(take(k, 0)() + 1, s.m() + 1);
