// sinh / tanh / asinh / atanh computed by subtracting two nearly
// equal numbers lose everything the answer is made of. At x = 2.3e-8
// — just past the 2^-26 guard these had — `(e - 1/e)/2` has both
// terms equal to 1 for sixteen digits, and what comes back is the
// remainder of them eating each other.
//
// Checked by properties rather than printed digits: an implementation
// is allowed its own last bit, but not eight of them.
function ulpsApart(a: number, b: number): number {
  if (a === b) { return 0 }
  const f = new Float64Array([a, b]);
  const u = new Uint32Array(f.buffer);
  const hi = u[3] - u[1];
  const lo = u[2] - u[0];
  return Math.abs(hi * 0x100000000 + lo);
}

const tiny = 2.3050326e-8;
// For x this small every one of these is x to within a couple of ULP.
console.log(
  ulpsApart(Math.sinh(tiny), tiny) <= 2,
  ulpsApart(Math.tanh(tiny), tiny) <= 2,
  ulpsApart(Math.asinh(tiny), tiny) <= 2,
  ulpsApart(Math.atanh(tiny), tiny) <= 2,
);

// Round trips, which the series forms broke outright. Kept below 1:
// past that `tanh` saturates and `atanh` magnifies its own input's
// last bits, so a wide round-trip measures the mathematics rather
// than the implementation.
let worst = 0;
for (const x of [1e-8, 1e-4, 0.01, 0.1, 0.5]) {
  worst = Math.max(worst, ulpsApart(Math.sinh(Math.asinh(x)), x));
  worst = Math.max(worst, ulpsApart(Math.atanh(Math.tanh(x)), x));
}
console.log("round-trip within 4 ulp:", worst <= 4);

// Signs and the identities that pin them.
console.log(1 / Math.sinh(-0), 1 / Math.tanh(-0), 1 / Math.asinh(-0), 1 / Math.atanh(-0));
console.log(Math.sinh(-2) === -Math.sinh(2), Math.tanh(-0.3) === -Math.tanh(0.3));
console.log(Math.expm1(-2) < 0, Math.expm1(-40) === -1, Math.log1p(-1));
