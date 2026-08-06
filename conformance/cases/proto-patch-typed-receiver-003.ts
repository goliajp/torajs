// RFC 20260806 — a patched Array higher-order method on a TYPED
// receiver, called with a NAMED function declaration as the callback.
//
// The callback shape is the point. Every probe that opened this gate
// the first time passed an arrow literal, which is already a closure;
// a bare function name is a raw FnSig, and an un-annotated one is an
// implicit generic. Both failed as whole-program rejects rather than
// wrong answers, which is worse than the bug being fixed, and 21
// test262 cases regressed on exactly this shape before it was covered.

function pick(v, i, o) {
  return v > 1;
}

function add(a, b) {
  return a + b;
}

const nums: number[] = [1, 2, 3];

// Un-patched first: the kernel answers, and the callback still has to
// reach it.
console.log(nums.some(pick), nums.every(pick), nums.filter(pick).length);
console.log(nums.reduce(add), nums.reduceRight(add));

(Array.prototype as any).some = function () {
  return "PATCHED-some";
};
(Array.prototype as any).every = function () {
  return "PATCHED-every";
};
(Array.prototype as any).filter = function () {
  return "PATCHED-filter";
};
(Array.prototype as any).reduce = function () {
  return "PATCHED-reduce";
};
(Array.prototype as any).map = function () {
  return "PATCHED-map";
};

// After the patch the same typed receiver must see it — the bitmap is
// read when the call runs, not when it is compiled.
console.log(nums.some(pick), nums.every(pick), nums.filter(pick));
console.log(nums.reduce(add), nums.map(pick));

// A string receiver stands down through the same gate.
const s: string = "abc";
console.log(s.toUpperCase());
(String.prototype as any).toUpperCase = function () {
  return "PATCHED-upper";
};
console.log(s.toUpperCase());
