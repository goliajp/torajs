// `c.v++` / `--c.v` on a class accessor: §13.4.4.1 reads through the
// getter, steps the number, and writes through the setter (PutValue).

class C {
  _v = 1;
  get v(): number { return this._v; }
  set v(x: number) { this._v = x * 10; }
}
const c = new C();
const a = c.v++;
console.log(a, c._v);
const b = ++c.v;
console.log(b, c._v);
const d = c.v--;
console.log(d, c._v);
console.log(--c.v, c._v);

class Base {
  n = 5;
  get k(): number { return this.n; }
  set k(x: number) { this.n = x + 100; }
}
class Sub extends Base {}
const s = new Sub();
s.k++;
console.log(s.n, s.k);

class RO {
  get r(): number { return 7; }
}
const ro: RO = new RO();
try {
  (ro as any).r++;
  console.log("no throw", ro.r);
} catch (e) {
  console.log((e as Error).constructor.name);
}

function bump(x: C): number {
  for (let i = 0; i < 3; i++) x.v++;
  return x._v;
}
console.log(bump(new C()));
