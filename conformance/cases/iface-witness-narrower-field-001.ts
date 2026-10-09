// A class declaring a data member with a narrower type than the
// interface's (`x: number` under `x: number | string`): its instances
// stand behind the interface and are read through it.
interface P {
  x: number | string;
}
class C {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const p: P = new C(4);
console.log(p.x, typeof p.x);
const q: P = { x: "lit" };
function show(v: P): string {
  return typeof v.x + ":" + v.x;
}
console.log(show(new C(7)), show(q));
const ps: P[] = [new C(1), q, new C(2)];
let out = "";
for (const v of ps) out += show(v) + " ";
console.log(out);
const c = new C(5);
const pc: P = c;
pc.x = 9;
console.log(pc.x, c.x);
// a wider value written through the interface is the class's field
// value too, read through either
pc.x = "s";
console.log(pc.x, c.x, typeof c.x, show(c));
c.x = 11;
console.log(pc.x, c.x + 1);

// a null written through the interface into a number field, and a
// field another class inherits
interface Nul {
  v: number | null;
  w: string | null;
}
class Base {
  v = 1;
  w = "w";
  twice(): number {
    return this.v === null ? -1 : this.v * 2;
  }
}
class Sub extends Base {
  u = 3;
}
const sub = new Sub();
const nul: Nul = sub;
nul.v = null;
nul.w = null;
console.log(nul.v, sub.v, nul.w, sub.w, sub.twice());
const asBase: Base = sub;
sub.v = 4;
console.log(nul.v, asBase.v, asBase.twice(), sub.u);

interface Q {
  id: number | null;
  name: string;
}
class Row {
  id: number;
  name: string;
  constructor(id: number, name: string) {
    this.id = id;
    this.name = name;
  }
}
class Pad {
  extra: boolean;
  id: number | null;
  name: string;
  constructor() {
    this.extra = true;
    this.id = null;
    this.name = "pad";
  }
}
function label(r: Q): string {
  return r.name + "#" + (r.id === null ? "none" : r.id);
}
const lq: Q = { id: 8, name: "lit" };
const rows: Q[] = [new Row(3, "a"), new Pad(), lq];
for (const r of rows) console.log(label(r));

// reached through another interface whose member is optional
interface HasY {
  y: number;
  m(): number;
}
interface MaybeY {
  y?: number;
}
class K {
  z = 1;
  y = 2;
  m() {
    return 3;
  }
}
const hy: HasY = new K();
const my: MaybeY = hy;
console.log(my.y, hy.m());

// a class whose member does not fit stays out, and is no witness
interface Num {
  n: number;
}
class Txt {
  n: string;
  constructor() {
    this.n = "t";
  }
}
const nn: Num = { n: 3 };
console.log(nn.n, new Txt().n);
