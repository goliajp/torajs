// Union type aliases (`type U = ...`): primitives, a leading `|` with
// one member per line, mixed literal kinds, aliases of aliases, and a
// union inside fields, maps and generic arguments.

type Prim = string | number;
type Wide = Prim | boolean;
function kind(w: Wide): string {
  return typeof w;
}
console.log(kind(1), kind("a"), kind(false));

type Tok =
  | number
  | string
  | boolean;
const toks: Tok[] = [1, "a", true];
console.log(toks.map((t) => typeof t).join(","));

type Mixed = "a" | 1;
let m: Mixed = 1;
console.log(m);
m = "a";
console.log(m);

type Lit = "a" | 1 | null;
const lits: Lit[] = ["a", 1, null];
console.log(lits.join(","));

type U = string | number | null;
let u: U = null;
console.log(u);
u = 5;
console.log(u);
u = "five";
console.log(u);

type Multi =
  | { t: 1 }
  | { t: 2 };
const mm: Multi = { t: 2 };
console.log(mm.t);

type Rec = { a: string | number; b?: boolean | string };
const r1: Rec = { a: 1 };
const r2: Rec = { a: "x", b: "y" };
console.log(r1.a, r1.b, r2.a, r2.b);

const mp = new Map<string, number | string>();
mp.set("a", 1);
mp.set("b", "two");
console.log(mp.get("a"), mp.get("b"));

class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}
const bb = new Box<Prim>(5);
console.log(bb.v);

function id<T>(x: T): T {
  return x;
}
console.log(id<Prim>(1), id<Prim>("s"));

class Holder {
  val: Prim = 0;
}
const h = new Holder();
h.val = "x";
console.log(h.val);
