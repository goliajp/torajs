// P11.1-S2-a acceptance — build-time encoding decision + ES `String.length`
// (UTF-16 code unit count) spec correctness.
//
// Runs the .length property on three encoding categories so a
// regression at any of Latin-1 baking, BMP UTF-16 baking, or
// surrogate-pair UTF-16 baking would surface in the bun byte-diff
// gate.
//
// Out of S2-a scope (queued for S3 / S4): non-ASCII concat,
// charCodeAt under UTF-16 dispatch, slice / substring under
// code-unit indexing. Those keep the pre-S2-a byte semantics until
// the runtime UTF-16 path lands.

console.log("abc".length);   // Latin-1 / ASCII subset → 3
console.log("中文".length);   // BMP UTF-16             → 2
console.log("😀".length);    // surrogate pair UTF-16  → 2
