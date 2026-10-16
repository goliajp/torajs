// `Reflect.set` writes the property "NaN" or "Infinity" the same way
// a keyed store does, so an object it leaves there has a valueOf a
// typed array's read at NaN / Infinity runs, and each caller's try
// sees it; so does `Reflect.set` called through a binding
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
function outer(xs: number[], k: number): number {
  const r = g(xs, k);
  console.log("outer went on", r);
  return r + 1;
}
const a: number[] = [1, 2];
Reflect.set(a, "NaN", {
  valueOf() {
    throw new Error("reflect NaN");
  },
});
const b: number[] = [3, 4];
Reflect.set(b, 1 / parseFloat("0"), {
  valueOf() {
    throw new Error("reflect Infinity");
  },
});
const c: number[] = [5, 6];
const put = Reflect.set;
put(c, "NaN", {
  valueOf() {
    throw new Error("held NaN");
  },
});
const d: number[] = [7, 8];
Reflect.set(d, "NaN", 9);
const nan = parseFloat("x");
const inf = 1 / parseFloat("0");
try {
  g(a, NaN);
  console.log("no throw");
} catch {
  console.log("caught");
}
try {
  console.log("value", outer(a, nan));
} catch (e) {
  console.log("caught outer", (e as Error).message);
}
try {
  console.log("value", g(b, inf));
} catch (e) {
  console.log("caught", (e as Error).message);
}
try {
  console.log("value", outer(c, nan));
} catch (e) {
  console.log("caught outer", (e as Error).message);
}
console.log("value", g(d, nan), g(a, 0.5), g(b, -inf), outer(a, 1));
console.log("done");
