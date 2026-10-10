// An object-element array read past the end goes on to the prototype
// chain (§10.1.8.1); a getter there runs, and its throw reaches the
// caller.
class Pt {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const ps: Pt[] = [new Pt(1), new Pt(2)];
let hits = 0;
Object.defineProperty(Array.prototype, "5", {
  get() {
    hits++;
    return undefined;
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "6", {
  get() {
    throw new Error("oob getter");
  },
  configurable: true,
});
const i = Number("5");
console.log(ps[i], hits, ps[1].x);
try {
  console.log(ps[i + 1]);
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(hits);
