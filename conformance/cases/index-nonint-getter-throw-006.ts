// a Record alias and an index-signature view of a number[] each write
// an own non-index property holding an object; the typed read's
// ToNumber runs its valueOf, which raises
type Bag = {
  [k: string]: object;
};
const a: number[] = [1, 2];
const r = a as any as Record<string, object>;
r["1.5"] = {
  valueOf() {
    throw new Error("record alias");
  },
};
function put(b: Bag, k: string) {
  b[k] = {
    valueOf() {
      throw new Error("index signature");
    },
  };
}
put(a as any, "2.5");
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
try {
  console.log(g(a, parseFloat("1.5")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
try {
  console.log(g(a, parseFloat("2.5")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(g(a, 1));
console.log("end");
