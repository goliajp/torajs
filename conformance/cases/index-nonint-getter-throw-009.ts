// a member store names the properties "NaN" and "Infinity", which a
// typed array's read at NaN / Infinity asks for; an object stored
// there through an `any` view has a valueOf that raises out of the
// read, and each caller's try sees it
function poke(o: any, v: any): void {
  o.NaN = v;
}
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
function outer(xs: number[], k: number): number {
  const r = g(xs, k);
  console.log("outer went on", r);
  return r + 1;
}
const a: number[] = [1, 2];
poke(a, {
  valueOf() {
    throw new Error("member NaN");
  },
});
const b: number[] = [3, 4];
const ys: any = b;
ys.Infinity = {
  valueOf() {
    throw new Error("member Infinity");
  },
};
const nan = parseFloat("x");
const inf = 1 / parseFloat("0");
try {
  console.log("value", g(a, nan));
} catch (e) {
  console.log("caught", (e as Error).message);
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
  console.log("value", outer(b, inf));
} catch (e) {
  console.log("caught outer", (e as Error).message);
}
console.log("value", g(a, 0.5), g(b, -inf), outer(a, 1));
console.log("done");
