// Sloppy code (a CommonJS script) discards a [[Set]] refusal on the
// typed lane: a frozen instance, a getter-only accessor and a field
// made non-writable keep their value and the assignment answers its
// right-hand side; a setter still runs (§13.15.2, §10.1.9.2).
class Cell { v: number; s: string; constructor(v: number) { this.v = v; this.s = "a"; } }
function setCell(c: Cell, v: number): number { c.v = v; return c.v; }
function setS(c: Cell, s: string): string { const r = (c.s = s + "!"); return r + "/" + c.s; }
const f = new Cell(1);
Object.freeze(f);
console.log(setCell(f, 2), f.v, setS(f, "x"), f.s);
const g = new Cell(1);
Object.defineProperty(g, "v", { get() { return 4; } });
console.log(setCell(g, 9), g.v);
const h = new Cell(1);
Object.defineProperty(h, "v", { writable: false });
console.log(setCell(h, 9), h.v);
const k = new Cell(1);
let seen = 0;
Object.defineProperty(k, "v", { get() { return seen; }, set(x: number) { seen = x * 3; } });
console.log(setCell(k, 5), seen);
