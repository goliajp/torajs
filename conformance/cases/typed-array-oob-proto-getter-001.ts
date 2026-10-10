// An integer key past the end, or below zero, is no own element; the
// read goes on to the prototype chain (§10.1.8.1), where a getter runs.
Object.defineProperty(Array.prototype, "5", {
  get() {
    return 42;
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "-1", {
  get() {
    return 43;
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "4294967296", {
  get() {
    return 44;
  },
  configurable: true,
});
Object.defineProperty(Array.prototype, "6", {
  get() {
    return "six";
  },
  configurable: true,
});
const xs: number[] = [1, 2, 3];
console.log(xs[5], xs[-1], xs[4294967296], xs[1]);
const ss: string[] = ["a"];
console.log(ss[6], ss[0], ss[7]);
const bs: boolean[] = [true];
console.log(bs[6], bs[0], bs[7]);
let i = 5;
console.log(xs[i], xs[i - 6]);
const ys: any = [1, 2];
console.log(ys[5], ys[-1], ys[4294967296]);
ys["-2"] = "own";
console.log(ys[-2]);
