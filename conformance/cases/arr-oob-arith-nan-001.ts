// An out-of-range read answers `undefined`, but the moment that
// value enters arithmetic the answer is a plain NaN — and it has to
// stay a plain NaN after a round trip through a container. The F64
// `undefined` sentinel is a NaN payload, and AArch64 hands a NaN
// operand's payload to the result, so without ToNumber(undefined)
// on the way in every one of these read back as `undefined`.
const zs: number[] = [1, 2, 3];
const p: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
p[0] = zs[9] + 1;
p[1] = zs[9] * 2;
p[2] = zs[9] / 2;
p[3] = zs[9] - 1;
p[4] = -(-zs[9]);
p[5] = Math.abs(zs[9]);
p[6] = Math.min(zs[9], 1);
p[7] = zs[9] % 2;
for (let i = 0; i < 8; i++) {
  console.log(i, p[i], p[i] === undefined, typeof p[i]);
}

// The array literal is the same round trip by another spelling.
const lit: number[] = [zs[9] + 1, zs[9]];
console.log(lit[0], lit[0] === undefined, lit[1], lit[1] === undefined);

// A genuine out-of-range read still answers `undefined` everywhere.
console.log(zs[9], zs[9] === undefined, typeof zs[9]);
console.log(zs[0], zs[0] === undefined);
console.log(isNaN(zs[9] + 1), zs[9] + 1 === undefined);
