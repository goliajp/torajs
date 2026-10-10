// A non-integral key on a boolean[] or an object-element array names a
// property (§7.1.19); a getter for it on Array.prototype runs, and its
// throw reaches the caller.
class Pt {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const ps: Pt[] = [new Pt(1), new Pt(2)];
const bs: boolean[] = [true, false];
console.log(ps[1.5], bs[1.5]);
Object.defineProperty(Array.prototype, "1.5", {
  get() {
    throw new Error("getter hit");
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "0.5", {
  get() {
    return "half";
  },
  configurable: true,
});
try {
  console.log(ps[1.5]);
} catch (e) {
  console.log("caught", (e as Error).message);
}
try {
  console.log(bs[1.5]);
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(bs[0.5], bs[2.5]);
