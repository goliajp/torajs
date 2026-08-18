// String.raw direct-call §22.1.2.4 — LengthOfArrayLike over the raw
// value (NaN / negative / absent length → ""), shape-blind element
// reads (array / string / dynobj array-like), nullish raw TypeError.
// Pre-fix the kernel read a non-array raw at an array layout offset
// — the t262 return-empty-string family's SIGSEGV.
console.log("[" + String.raw({ raw: { length: NaN } }) + "]");
console.log("[" + String.raw({ raw: { length: -Infinity } }) + "]");
console.log("[" + String.raw({ raw: {} }) + "]");
console.log("[" + String.raw({ raw: 42 }) + "]");
console.log("[" + String.raw({ raw: [] }) + "]");
const like: any = { length: 2, 0: "A", 1: "B" };
console.log("[" + String.raw({ raw: like }, "-") + "]");
console.log(String.raw({ raw: "xy" }, 7));
console.log(String.raw({ raw: ["a", "b", "c"] }, 1, 2));
try {
  String.raw({});
} catch (e) {
  console.log("caught-missing-raw");
}
try {
  String.raw({ raw: null });
} catch (e) {
  console.log("caught-null-raw");
}
const n: any = 5;
console.log(String.raw`a${n}b`);
console.log(String.raw`\n${n}\t`);
console.log("done");
