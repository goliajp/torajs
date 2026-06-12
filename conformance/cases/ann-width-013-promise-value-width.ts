// W3 chunk 2.6b (F5) — promise value-slot width negotiation. The
// promise runtime moves the resolved value as raw 8 bytes and calls
// handlers through one fixed (env, i64) -> i64 signature; before
// 2.6b an f64 face anywhere on the chain crossed that boundary in
// the wrong register bank — `then(v => v / 2)` returned a garbage
// pointer-sized integer and `resolve(2.5)` corrupted the value.
// All shapes here trigger on plain division / fractional literals —
// live silent-wrongs independent of the S9 mul flip.

// resolve f64 → await (write-side bits + read-side decode).
console.log(await Promise.resolve(2.5));  // 2.5

// integral promise keeps the narrow slot.
console.log(await Promise.resolve(7));  // 7

// through a let binding (congruence with the Anon origin).
let p = Promise.resolve(1.25);
console.log(await p);  // 1.25

// cb ret floats: the thunk decodes bits in, encodes bits out.
console.log(await Promise.resolve(7).then((v: number) => v / 2));  // 3.5

// f64 source into the cb param face.
console.log(await Promise.resolve(2.5).then((v: number) => v + 1));  // 3.5

// chained then — the result point passes through.
console.log(
  await Promise.resolve(8)
    .then((v: number) => v / 2)
    .then((v: number) => v + 0.25),
);  // 4.25

// named-fn handler (FnSig variant of the adapter).
function halve(x: number): number {
  return x / 2;
}
console.log(await Promise.resolve(9).then(halve));  // 4.5

// rejected value through catch.
console.log(await Promise.reject(2.5).catch((r: number) => r * 2));  // 5

// 2-arg then(onOk, onErr).
console.log(await Promise.resolve(5).then(
  (v: number) => v / 2,
  (e: number) => e,
));  // 2.5

// integral chain stays narrow end to end.
console.log(await Promise.resolve(6).then((v: number) => v + 1));  // 7
