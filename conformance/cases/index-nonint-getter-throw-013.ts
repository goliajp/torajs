// `Reflect.set(target, key, value, receiver)` defines the property on
// the receiver (§10.1.9.2), so an array passed there holds the object
// at "NaN"; a typed array's read at NaN runs its valueOf, and the
// caller's try sees it
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
const d: number[] = [7, 8];
const holder = new Map<string, number>();
const ok = Reflect.set(
  holder,
  "NaN",
  {
    valueOf() {
      throw new Error("receiver argument");
    },
  },
  d,
);
try {
  console.log("value", g(d, parseFloat("x")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(ok, holder.size, g(d, 0));
