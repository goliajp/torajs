// A callback over an escaped typed array that writes a binding outside
// it: the element reaches the callback as it is (its all-any twin),
// and the binding stores the sum as it comes out. A typed array that
// never escaped keeps the typed callback and the number binding.
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";

let tot: number = 0;
d.forEach(v => { tot = tot + v; });
console.log("module", tot, typeof tot);

function sumIn(xs: number[]): void {
  let inner: number = 0;
  const kinds: string[] = [];
  xs.forEach(v => { inner = inner + v; kinds.push(typeof v); });
  console.log("captured", inner, typeof inner, kinds.join(","));
}
sumIn(d);

let count = 0;
d.forEach((v: number) => { count = count + v * 1; });
console.log("arith", count, typeof count);

const b: number[] = [4, 5, 6];
let tb: number = 0;
b.forEach(v => { tb = tb + v; });
console.log("plain", tb, typeof tb);
