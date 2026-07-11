// any-method dispatch backfill chunk 3 — Array.prototype.every /
// some / find / findIndex / reduce / reduceRight through an `any`
// receiver. The predicate quartet shares an early-exit loop (find
// transfers the owned element read as its return); reduce walks the
// 4-arg accumulator fold, seeds from the first walked element when
// initialValue is absent (argc question, not undefined), and the
// empty-array no-init case raises the spec TypeError through the
// existing throw_empty sentinels.
//
// Acceptance: byte-equal with bun.

const a = [1, 2, 3, 4, 5] as any;
console.log(a.every((x: any) => x > 0));
console.log(a.every((x: any) => x > 2));
console.log(a.some((x: any) => x > 4));
console.log(a.some((x: any) => x > 9));
console.log(a.find((x: any) => x > 2));
console.log(a.find((x: any) => x > 9));
console.log(a.findIndex((x: any) => x > 2));
console.log(a.findIndex((x: any) => x > 9));
console.log(a.reduce((acc: any, x: any) => acc + x));
console.log(a.reduce((acc: any, x: any) => acc + x, 100));
console.log(a.reduceRight((acc: any, x: any) => acc - x));
console.log(a.reduceRight((acc: any, x: any) => acc - x, 100));

// heap elements + mixed Arr<Any>
const s: any = ["a", "b", "c"];
console.log(s.reduce((acc: any, x: any) => acc + x));
console.log(s.find((x: any) => x === "b"));
console.log(s.every((x: any) => x.length === 1));
const m: any = [1, "x", true, null];
console.log(m.some((x: any) => x === null));
console.log(m.findIndex((x: any) => x === "x"));

// (value, index, array) callback shape
const idx = [10, 20, 30] as any;
console.log(idx.every((x: any, i: number, arr: any) => arr[i] === x));
console.log(idx.reduce((acc: any, x: any, i: any) => acc + i, 0));

// empty-array edges + the spec TypeError
const e = [] as any;
console.log(e.every((x: any) => false));
console.log(e.some((x: any) => true));
console.log(e.find((x: any) => true));
console.log(e.findIndex((x: any) => true));
console.log(e.reduce((acc: any, x: any) => acc, 7));
try {
  e.reduce((acc: any, x: any) => acc);
} catch (err: any) {
  console.log("caught:", err.message);
}
try {
  e.reduceRight((acc: any, x: any) => acc);
} catch (err: any) {
  console.log("caught:", err.message);
}
