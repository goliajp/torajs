// RFC-less knife (rotation 349) — §23.1.3 integer-limit early-exit
// gates on the array-like lanes: ArraySpeciesCreate/ArrayCreate caps
// at 2^32-1 (RangeError) for map/slice/toReversed/with/toSpliced and
// the splice removed-product; the post-mutation length caps at
// 2^53-1 (TypeError) for push/unshift/splice. All O(1) — the clamp
// previously walked ~2^53 indexes, which read as a hang. Messages
// match JSC per-method.
const big: any = { length: Infinity };
const big32: any = { length: 4294967296 };
function t(label: string, fn: () => void) {
  try { fn(); console.log(label, "| no-throw"); }
  catch (e) { console.log(label, "|", (e as any).name, "|", (e as any).message); }
}
t("map", () => (Array.prototype.map as any).call(big32, (x: any) => x));
t("slice", () => (Array.prototype.slice as any).call(big32));
t("toReversed", () => (Array.prototype.toReversed as any).call(big32));
t("with", () => (Array.prototype.with as any).call(big32, 0, 1));
t("toSpliced", () => (Array.prototype.toSpliced as any).call(big32, 0, 0));
t("unshift", () => (Array.prototype.unshift as any).call(big, 1));
t("push", () => (Array.prototype.push as any).call(big, 1));
t("splice-ins", () => (Array.prototype.splice as any).call(big, 0, 0, null));
t("splice-del", () => (Array.prototype.splice as any).call(big32, 0));
