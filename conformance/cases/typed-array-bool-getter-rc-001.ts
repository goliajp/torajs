// A getter reached by a boolean[] read past the end, or at a
// non-integral key, hands its object to the read; each read releases
// it, so the loop runs in flat memory.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
let made = 0;
Object.defineProperty(Array.prototype, "3", {
  get() {
    made++;
    return new Box(made);
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "0.5", {
  get() {
    made++;
    return new Box(-made);
  },
  configurable: true,
});
const bs: boolean[] = [true, false];
const n = Number(process.argv[2] ?? "1000");
let sum = 0;
const i = Number("3");
const h = Number("0.5");
for (let r = 0; r < n; r++) {
  const v: any = bs[i];
  sum += (v as Box).v;
  const w: any = bs[h];
  sum += (w as Box).v;
}
console.log(made, sum, bs[0], bs[1]);
