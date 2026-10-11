// Strict refusals on the typed store (§10.1.9.2, §13.15.2): a frozen
// instance, a field made non-writable and a getter-only accessor all
// throw TypeError and keep their value, while a sealed instance still
// takes data writes. An accessor on the class prototype does not reach
// the declared field, which is an own data property of each instance
// (§7.3.33 DefineField runs for it when the instance is built).
class Cell { v: number; constructor(v: number) { this.v = v; } }
function setCell(c: Cell, v: number): number { c.v = v; return c.v; }

const f = new Cell(1);
Object.freeze(f);
try { console.log(setCell(f, 2)); } catch (e) { console.log("frozen threw", e instanceof TypeError, f.v); }
const s = new Cell(1);
Object.seal(s);
console.log(setCell(s, 2), s.v);
const w = new Cell(1);
Object.defineProperty(w, "v", { writable: false });
try { console.log(setCell(w, 2)); } catch (e) { console.log("ro threw", e instanceof TypeError, w.v); }
const g = new Cell(3);
Object.defineProperty(g, "v", { get() { return 1; } });
try { console.log(setCell(g, 6)); } catch (e) { console.log("getter-only threw", e instanceof TypeError, g.v); }

let calls = 0;
class Pt { x: number; constructor(x: number) { this.x = x; } }
Object.defineProperty(Pt.prototype, "x", {
  get() { calls++; return -1; },
  set(_v: number) { calls++; },
  configurable: true,
});
function setX(p: Pt, x: number): number { p.x = x; return p.x; }
const p = new Pt(3);
console.log(setX(p, 6), p.x, calls, Object.keys(p).join());
