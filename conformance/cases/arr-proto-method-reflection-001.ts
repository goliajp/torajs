// RFC 20260712-array-generic-receiver chunk 1 — Array any-tier
// method surface catch-up: new mids flat / flatMap / findLast /
// findLastIndex / toReversed / toSorted / toSpliced / with plus the
// at / toString dispatch arms the reified-cell surface was missing.
// Reflection face (typeof / .name / .length off Array.prototype)
// resolves through the interned method-cell table; behavior arms
// compose the kind-aware copy kernels (flat depth loop terminates
// on Infinity through the nested scan; toSpliced seeds a plain
// Array<Any> copy so foreign items never trip the typed admit).
//
// Acceptance: byte-equal with bun.

// reflection — the ES2019/ES2023 additions mint reified cells
console.log(typeof (Array.prototype as any).at);
console.log(typeof (Array.prototype as any).flat);
console.log(typeof (Array.prototype as any).flatMap);
console.log(typeof (Array.prototype as any).findLast);
console.log(typeof (Array.prototype as any).findLastIndex);
console.log(typeof (Array.prototype as any).toReversed);
console.log(typeof (Array.prototype as any).toSorted);
console.log(typeof (Array.prototype as any).toSpliced);
console.log(typeof (Array.prototype as any).with);
console.log(typeof (Array.prototype as any).toString);
const at: any = (Array.prototype as any).at;
console.log(at.length, at.name);
const tsp: any = (Array.prototype as any).toSpliced;
console.log(tsp.length, tsp.name);

// flat — depth default 1 / explicit / Infinity / 0-copy
const nested: any = [1, [2, [3, [4]]], 5];
console.log(nested.flat());
console.log(nested.flat(2));
console.log(nested.flat(Infinity));
console.log(nested.flat(0));

// flatMap / findLast / findLastIndex / at / toString
const b: any = [1, 2, 3];
console.log(b.flatMap((x: any) => [x, x * 2]));
console.log(b.findLast((x: any) => x < 3));
console.log(b.findLastIndex((x: any) => x < 3));
console.log(b.findLast((x: any) => x > 9));
console.log(b.findLastIndex((x: any) => x > 9));
console.log(b.at(-1), b.at(0), b.at(9));
console.log(b.toString());

// change-array-by-copy — receiver stays untouched
console.log(b.toReversed(), b);
console.log(([3, 1, 2] as any).toSorted());
console.log(([3, 1, 2] as any).toSorted((x: any, y: any) => y - x));
console.log(([1, 2, 3, 4] as any).toSpliced(1, 2, "a"));
console.log(b.with(1, "x"), b);

// heap elements through the copy family (rc ledger)
const strs: any = ["pear", "apple", "fig"];
console.log(strs.toReversed());
console.log(strs.toSorted());
console.log(strs.with(0, "kiwi"), strs);

// detached .call re-dispatch on a real array receiver
const arr: any = [10, 20, 30];
console.log((Array.prototype as any).at.call(arr, -1));
console.log((Array.prototype as any).toReversed.call(arr));
console.log((Array.prototype as any).flat.call([[7], 8] as any));

// with OOB — catchable RangeError
try {
  (arr as any).with(9, 1);
} catch (e: any) {
  console.log("caught", e instanceof RangeError);
}
