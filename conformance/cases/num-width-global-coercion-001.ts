// The three global functions that answer a Number — §19.2.4
// parseFloat, §19.2.5 parseInt, §21.1.1 Number — have to give the
// width analysis an f64 answer, because each of them can answer NaN
// or a fraction and an integer slot cannot hold either.
//
// The gap was invisible without an explicit annotation: an
// unannotated binding takes its width from the initializer, so only
// `const n: number = Number(s)` reached the bad slot, and it did not
// compile at all — the register allocator refused an FPR value where
// the consumer wanted a GPR.
const s: any = "7";
const a: number = Number(s);
const b: number = parseInt("42px");
const c: number = parseFloat("3.5rem");
console.log(a, b, c);
console.log(a + b + c, a < b, c > 1, a === 7);

// NaN is the value an integer slot cannot hold.
const bad: number = Number("zz");
console.log(bad, bad + 1, Number.isNaN(bad), bad < 1, bad > 1);
const frac: number = parseFloat("0.25");
console.log(frac, frac * 4, frac < 1);

// The loop bound is where it showed up in practice.
let total: number = 0;
for (let i = 0; i < a; i++) { total = total + i; }
console.log(total);

// Inside a function, and as a return value.
function twice(p: any): number {
  const n: number = Number(p);
  return n * 2;
}
console.log(twice(4), twice("5"), twice(1.5), twice(true));

// In an array literal and through a radix.
const arr: number[] = [Number("1"), parseInt("2"), parseFloat("3")];
console.log(arr, arr[0] + 1);
console.log(parseInt("10", 2), parseInt("ff", 16), parseInt("0.9"));

// A user function of the same name keeps its own return width — the
// arm sits after the declared-function lookup for exactly this.
function parseFloatish(x: string): number { return x.length; }
const shadowed: number = parseFloatish("abcd");
console.log(shadowed, shadowed + 0.5);
