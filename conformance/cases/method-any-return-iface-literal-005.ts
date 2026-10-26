// A method that returns an any value, reached through one type spelled
// several ways: an alias of the return type, a function-typed property
// beside method syntax, an alias of an object type, and object type
// literals in each spelling, also crossed with the interface's own
// (`k(): Num` against `k(): number`, `q(): number` against
// `q: () => Num`). Every spelling takes the class instances and object
// literals and hands the value back as it is.
const raw: any = JSON.parse('{"x": 1.5, "y": "s"}');

type Num = number;
interface I {
  m(): Num;
}
interface J {
  m: () => number;
}
type Shape = { m(): number };
class P implements I, J {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.y;
  }
}
function viaI(o: I) {
  return o.m();
}
function viaJ(o: J) {
  return o.m();
}
function viaShape(o: Shape) {
  return o.m();
}
function viaAliasLit(o: { m(): Num }) {
  return o.m();
}
function viaPropLit(o: { m: () => number }) {
  return o.m();
}
const p = new P(raw);
console.log(viaI(p), viaJ(p), viaShape(p), viaAliasLit(p), viaPropLit(p));
const lit = { m: () => 2.5 };
console.log(
  viaI(lit),
  viaJ(lit),
  viaShape(lit),
  viaAliasLit(lit),
  viaPropLit(lit),
);
const short: Shape = {
  m() {
    return 4;
  },
};
console.log(viaI(short), viaJ(short), viaPropLit(short) + 1);
const xs: J[] = [p, lit, short];
for (const x of xs) console.log(x.m());

interface K {
  k(): number;
  n: number;
}
interface Q {
  q: () => Num;
}
class R implements K, Q {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  k(): number {
    return this.v.x;
  }
  q(): Num {
    return this.v.y;
  }
}
function crossK(o: { k(): Num; n: Num }) {
  return o.k();
}
function crossK2(o: { k: () => number; n: number }) {
  return o.k();
}
function crossQ(o: { q(): number }) {
  return o.q();
}
const r = new R(raw);
console.log(crossK(r), crossK2(r), crossQ(r));
console.log(
  crossK({ k: () => 6, n: 0 }),
  crossK2({ k: () => 7, n: 0 }),
  crossQ({ q: () => 8 }),
);
