// Integrity levels and redefinitions around the typed store: another
// field's redefinition leaves this one plain, freeze refuses after an
// accessor install, seal keeps data writes, a deleted field revives on
// write, an accessor turned back into data is a plain field again.
class Cell { v: number; w: number; constructor(v: number) { this.v = v; this.w = 0; } }
function setCell(c: Cell, v: number): number { c.v = v; c.w = v + 1; return c.v + c.w; }
// another field redefined: this instance is exotic, `v` is still plain data
const a = new Cell(1);
Object.defineProperty(a, "w", { enumerable: false });
console.log(setCell(a, 4), a.v, a.w, Object.keys(a).join());
// frozen after an accessor install
const b = new Cell(1);
let hits = 0;
Object.defineProperty(b, "v", { get() { return 5; }, set(x: number) { hits += x; } });
Object.freeze(b);
try { console.log(setCell(b, 2)); } catch (e) { console.log("threw", e instanceof TypeError, hits); }
// sealed: data writes still go through
const c = new Cell(1);
Object.seal(c);
console.log(setCell(c, 3), Object.isSealed(c));
// delete, then write revives
const d = new Cell(1);
delete (d as any).w;
console.log(setCell(d, 5), d.w, Object.keys(d).join());
// accessor turned back into data
const e = new Cell(1);
Object.defineProperty(e, "v", { get() { return 50; }, configurable: true });
console.log(e.v);
Object.defineProperty(e, "v", { value: 7, writable: true });
console.log(e.v, setCell(e, 8), e.v);
