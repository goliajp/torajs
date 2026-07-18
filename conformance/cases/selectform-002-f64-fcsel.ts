// f64 select formation (RFC 20260719 residual "FCSEL + F64 面"): the
// arms of a pure diamond may be f64, in which case the emitter picks
// FCSEL for the value arms while the condition stays on a GPR (`cmp
// cond, #0; fcsel d, dn, dm, ne`). Register class follows each
// *value's* declared type, so no allocator change is involved.
//
// Covers: constant arms, computed arms, a spilled-heavy arm chain,
// negative zero (which csel must move bit-exactly, not renormalize),
// and NaN propagation through the selected arm.
function pick(a: number, b: number): number {
  let m: number = 0.0;
  if (a > b) {
    m = a * 1.5;
  } else {
    m = b * 0.5;
  }
  return m;
}

let s: number = 0.0;
let i: number = 0;
while (i < 1000) {
  s = s + pick(i * 0.5, 250.0);
  i = i + 1;
}
console.log(s);

// constant arms — both sides materialize through the GPR carrier
function sign(x: number): number {
  let r: number = 0.0;
  if (x < 0.0) {
    r = -1.5;
  } else {
    r = 2.25;
  }
  return r;
}
console.log(sign(-3.0));
console.log(sign(3.0));

// -0.0 must survive the move bit-exactly: 1/-0 is -Infinity, 1/0 is
// +Infinity, so a renormalizing move would show up here.
function zpick(c: boolean): number {
  let z: number = 0.0;
  if (c) {
    z = -0.0;
  } else {
    z = 0.0;
  }
  return z;
}
console.log(1 / zpick(true));
console.log(1 / zpick(false));

// NaN rides the selected arm untouched
function npick(c: boolean): number {
  let n: number = 0.0;
  if (c) {
    n = NaN;
  } else {
    n = 7.5;
  }
  return n;
}
console.log(npick(true));
console.log(npick(false));

// many live f64 values across the diamond — pushes arms onto spill
// slots so the LDR-reload path into the FCSEL operands gets exercised
function wide(a: number, b: number, c: number, d: number, e: number): number {
  const p: number = a * 1.1;
  const q: number = b * 1.2;
  const r: number = c * 1.3;
  const t: number = d * 1.4;
  const u: number = e * 1.5;
  let m: number = 0.0;
  if (p > q) {
    m = r + t;
  } else {
    m = t + u;
  }
  return p + q + r + t + u + m;
}
console.log(wide(1.0, 2.0, 3.0, 4.0, 5.0));
console.log(wide(9.0, 2.0, 3.0, 4.0, 5.0));
