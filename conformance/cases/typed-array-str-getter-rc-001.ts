// A getter reached by a string[] read past the end, or at a
// non-integral key, computes a fresh string; each read releases it,
// so the loop runs in flat memory.
let made = 0;
Object.defineProperty(Array.prototype, "4", {
  get() {
    made++;
    return "s" + made;
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "1.5", {
  get() {
    made++;
    return "h" + made;
  },
  configurable: true,
});
const ss: string[] = ["a", "b"];
const n = Number(process.argv[2] ?? "1000");
let len = 0;
const i = Number("4");
const h = Number("1.5");
let last = "";
for (let r = 0; r < n; r++) {
  const v = ss[i];
  len += v.length;
  const w = ss[h];
  len += w.length;
  last = ss[r % 2];
}
console.log(made, len, last, ss[0], ss[1], ss[i]);
