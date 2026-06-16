// S132 narrow — `Array<T>.reduceRight(fn, init)` walks last → first
// (spec §22.1.3.22). ssa-lower shares the M6.2 loop scaffold with
// `reduce`, differing only in the cursor's init (`len-1`), cmp
// (`i > -1`), and decrement (`i - 1`). Same callback `(acc, x) → T`
// + initial T signature as `reduce`.

// number[] — basic accumulation
const ns: number[] = [1, 2, 3, 4];
console.log(ns.reduceRight((a, b) => a + b, 0));    // 10
console.log(ns.reduceRight((a, b) => a - b, 100));  // 100-4-3-2-1 = 90

// Order-dependent reduction: build a string showing visit order
const ss: string[] = ["a", "b", "c", "d"];
console.log(ss.reduceRight((acc, x) => acc + x, ""));  // "dcba"

// Empty array — initial value is returned unchanged
const es: number[] = [];
console.log(es.reduceRight((a, b) => a + b, 42));   // 42

// regression — plain reduce still walks first → last
const xs: string[] = ["x", "y", "z"];
console.log(xs.reduce((acc, c) => acc + c, ""));    // "xyz"
console.log(xs.reduceRight((acc, c) => acc + c, "")); // "zyx"
