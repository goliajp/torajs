// Every spelling that can install an accessor reaches the typed field
// read and store: a key held in a variable (Object and Reflect) and a
// key computed at run time.
class Cell {
  a: number; b: number; c: number; d: number;
  constructor() { this.a = 1; this.b = 2; this.c = 3; this.d = 4; }
}
function total(x: Cell): number { return x.a + x.b * 10 + x.c * 100 + x.d * 1000; }
function setAll(x: Cell, v: number): void { x.a = v; x.b = v; x.c = v; x.d = v; }

const keys = ["a", "b"];
const x = new Cell();
Object.defineProperty(x, keys[0], { get() { return 7; } });
Reflect.defineProperty(x, keys[1], { get() { return 8; } });
console.log(total(x), total(new Cell()));

const y = new Cell();
let set = 0;
const which = String.fromCharCode(99);
Object.defineProperty(y, which, { get() { return 9; }, set(v: number) { set = v; } });
setAll(y, 5);
console.log(total(y), set);
