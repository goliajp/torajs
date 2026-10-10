// a getter for "1.5" on Array.prototype raises out of a typed array's
// non-integral index read, and the caller's try sees it
Object.defineProperty(Array.prototype, "1.5", {
  get() {
    throw new Error("getter " + typeof this);
  },
});
function h(xs: number[], k: number): number {
  return xs[k];
}
function g(xs: number[], k: number): number {
  return xs[k] + Math.abs(k);
}
function hs(xs: string[], k: number): string {
  return xs[k];
}
function ho(xs: number[] | undefined, k: number): number | undefined {
  return xs?.[k];
}
function sum(xs: number[]): number {
  let t = 0;
  for (let i = 0; i < xs.length; i++) t += xs[i];
  return t;
}
const a: number[] = [1, 2];
const s: string[] = ["a", "b"];
const k = parseFloat("1.5");
try {
  console.log(h(a, k));
} catch (e) {
  console.log("h caught", (e as Error).message);
}
try {
  console.log(g(a, k));
} catch (e) {
  console.log("g caught", (e as Error).message);
}
try {
  console.log(hs(s, k));
} catch (e) {
  console.log("hs caught", (e as Error).message);
}
try {
  console.log(ho(a, k));
} catch (e) {
  console.log("ho caught", (e as Error).message);
}
// an integer literal past 2^53 is a double; past 2^63 the read goes
// keyed and names "1e+300" / "1.2089258196146292e+24"
Object.defineProperty(Array.prototype, "1e+300", {
  get() {
    throw new Error("big");
  },
});
Object.defineProperty(Array.prototype, "1.2089258196146292e+24", {
  get() {
    throw new Error("mid");
  },
});
function big(xs: number[]): number {
  return xs[1e300];
}
function mid(xs: number[]): number {
  return xs[1208925819614629174706176];
}
try {
  console.log(big(a));
} catch (e) {
  console.log("big caught", (e as Error).message);
}
try {
  console.log(mid(a));
} catch (e) {
  console.log("mid caught", (e as Error).message);
}
console.log(h(a, 1), hs(s, 0), ho(undefined, k), sum(a));
console.log("end");
