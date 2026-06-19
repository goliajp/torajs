// ES §22.1.3.21 step 3 / §22.1.3.22 step 3 — `[].reduce(fn)` and
// `[].reduceRight(fn)` with no `initialValue` throw TypeError.
// Non-empty (single elem, no fn call) and explicit-init paths
// remain unchanged.

// Empty typed Number array — reduce should throw.
const e1: number[] = [];
try {
  const r = e1.reduce((acc, v) => acc + v);
  console.log("no throw, r=", r);
} catch (e) {
  console.log("threw:", (e as Error).message);
}

// Empty typed Number array — reduceRight should throw too.
const e2: number[] = [];
try {
  const r = e2.reduceRight((acc, v) => acc + v);
  console.log("no throw, r=", r);
} catch (e) {
  console.log("threw:", (e as Error).message);
}

// Empty typed String array — reduce should throw with same message.
const e3: string[] = [];
try {
  const r = e3.reduce((acc, v) => acc + v);
  console.log("no throw, r=", r);
} catch (e) {
  console.log("threw:", (e as Error).message);
}

// Explicit initialValue on empty — no throw, returns init.
const e4: number[] = [];
const r4 = e4.reduce((acc, v) => acc + v, 100);
console.log("init:", r4);

// Single-elem no-init — returns the element, fn never called.
const s1 = [42];
const r5 = s1.reduce((acc, v) => {
  console.log("fn called");
  return acc + v;
});
console.log("single:", r5);

// Non-empty fold no-init — fn called len-1 times.
const a = [1, 2, 3, 4];
const r6 = a.reduce((acc, v) => acc + v);
console.log("sum:", r6);
const r7 = a.reduceRight((acc, v) => acc - v);
console.log("rdiff:", r7);
