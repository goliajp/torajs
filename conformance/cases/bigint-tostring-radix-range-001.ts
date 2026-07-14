// ES §6.1.6.2.13 — BigInt.prototype.toString(radix) with radix outside
// [2, 36] is a RangeError. This was a debug_assert (a release no-op),
// so (255n).toString(1) drove radix_chunk's `chunk *= radix` loop
// (never overflows a u64 for radix < 2) into a non-terminating spin.
function r(f: () => any): string {
  try {
    f();
    return "NO THROW";
  } catch (e: any) {
    return e.name;
  }
}
console.log(r(() => (255n).toString(1)));
console.log(r(() => (255n).toString(0)));
console.log(r(() => (255n).toString(37)));
console.log((255n).toString(16));
console.log((255n).toString(2));
console.log((255n).toString());
