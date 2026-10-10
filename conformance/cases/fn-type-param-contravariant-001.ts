// a function value whose parameter is a wider object type fits a
// function-typed slot whose parameter is narrower (parameters are
// contravariant)
class Base {
  tag: string;
  constructor(tag: string) { this.tag = tag; }
  name(): string { return "base:" + this.tag; }
}
class Derived extends Base {
  n: number;
  constructor(tag: string, n: number) { super(tag); this.n = n; }
  name(): string { return "derived:" + this.tag + ":" + this.n; }
}

function viaBase(b: Base): string { return b.name() + "/" + b.tag; }
const holder: (d: Derived) => string = viaBase;
console.log(holder(new Derived("x", 3)));

function apply(f: (d: Derived) => string, d: Derived): string { return f(d); }
console.log(apply(viaBase, new Derived("y", 4)));

interface Narrow { a: number }
interface Wide { a: number; b: string }
const show = (n: Narrow): void => { console.log("a=" + n.a); };
const g: (w: Wide) => void = show;
g({ a: 1, b: "x" });

let h: (w: Wide) => void = (w: Wide): void => { console.log("wide " + w.b); };
h({ a: 2, b: "y" });
h = show;
h({ a: 5, b: "z" });

const fs: Array<(d: Derived) => string> = [viaBase, (d: Derived): string => "own:" + d.n];
for (const f of fs) console.log(f(new Derived("z", 9)));
