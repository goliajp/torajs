// Sloppy code discards a [[Set]] refusal on the typed lane (§13.15.2):
// the refused string is released whether the assignment's value is
// dropped (an expression statement) or read, and the field keeps its
// value.
class Cell { s: string = "a"; t: string = "b"; }
const c = new Cell();
Object.freeze(c);
const base = "x".repeat(40);
let len = 0;
for (let i = 0; i < 1000; i++) {
  c.s = base + i;
  const r = (c.t = base + "?" + i);
  len += r.length;
}
console.log(c.s, c.t, len);
const d = new Cell();
Object.defineProperty(d, "s", { get() { return "g"; } });
for (let i = 0; i < 1000; i++) {
  d.s = base + i;
}
console.log(d.s, d.t);
