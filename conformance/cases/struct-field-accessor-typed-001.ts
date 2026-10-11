// A declared field turned into an accessor by defineProperty is read
// and written through its getter / setter on the typed lane too
// (§10.1.8.1 step 7, §10.1.9.2 step 3): plain store, plain load,
// compound assignment, update expressions, destructuring, `this.v`
// in a method. An instance nobody redefined keeps its plain field.
class Cell {
  v: number;
  constructor(v: number) { this.v = v; }
  bump(): number { this.v = this.v + 1; return this.v; }
}
function setCell(c: Cell, v: number): number { c.v = v; return c.v; }
function getV(c: Cell): number { return c.v; }
function inc(c: Cell): number { c.v += 1; c.v++; ++c.v; return c.v; }
function pair(c: Cell): string { const { v } = c; return "v=" + v; }

const r = new Cell(3);
let seen = 0;
Object.defineProperty(r, "v", { get() { return seen; }, set(x: number) { seen = x * 10; } });
console.log(setCell(r, 6), seen);

const g = new Cell(3);
let n = 0;
Object.defineProperty(g, "v", { get() { n++; return 40 + n; } });
console.log(getV(g), g.v, n, pair(g), n);

const log: string[] = [];
let cur = 5;
const h = new Cell(3);
Object.defineProperty(h, "v", {
  get() { log.push("g" + cur); return cur; },
  set(x: number) { log.push("s" + x); cur = x * 2; },
});
console.log(inc(h), log.join(","));

let m = 10;
const k = new Cell(3);
Object.defineProperty(k, "v", { get() { return m; }, set(x: number) { m = x * 100; } });
console.log(k.bump(), m);

const plain = new Cell(3);
console.log(setCell(plain, 6), getV(plain), inc(plain), pair(plain), plain.bump());
