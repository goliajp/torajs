// §23.2 Float16Array — IEEE-754 binary16, whose whole content is
// what happens to a value that does not fit in eleven significant
// bits.
console.log(typeof Float16Array, Float16Array.name, Float16Array.BYTES_PER_ELEMENT, Float16Array.length);
const f = new Float16Array(4);
console.log(f, f.length, f.byteLength, f.byteOffset);
console.log(Object.prototype.toString.call(f));
console.log(f instanceof Float16Array, f instanceof Float32Array, ArrayBuffer.isView(f));
console.log(new Float16Array([1, 2, 3]).constructor === Float16Array);

// Exactly representable values survive untouched.
console.log(new Float16Array([0, 1, -1, 0.5, 0.25, 2, 1024, 65504, -65504]));

// Everything else rounds to nearest, TIES TO EVEN — which is the
// half of the spec a truncating stand-in gets wrong and nothing else
// notices. 2049 sits exactly between 2048 and 2052 and picks the
// even one; 2051 is simply nearer 2052.
console.log(new Float16Array([2049, 2050, 2051, 4098]));
console.log(new Float16Array([0.1])[0] === 0.0999755859375);
console.log(new Float16Array([1 / 3])[0]);

// The tie between the largest normal and 2^16 rounds AWAY, so 65520
// is already infinity while 65519 is still 65504.
console.log(new Float16Array([65519, 65520, 1e30, -1e30]));
console.log(new Float16Array([Infinity, -Infinity, NaN]));

// Subnormals, including the one that rounds up into the normals.
console.log(new Float16Array([1 / 16777216, 1023 / 16777216, 1023.5 / 16777216, 1024 / 16777216]));
// Below half the smallest subnormal is zero, and the sign is kept.
const z = new Float16Array([-0, 1e-30, -1e-30]);
console.log(z, 1 / z[0], 1 / z[1], 1 / z[2]);

// It is a view like any other, and a whole-buffer one divides by 2.
const b = new ArrayBuffer(8);
const view = new Float16Array(b);
console.log(view.length, view.byteLength, view.buffer === b);
view[0] = 1;
console.log(new Uint16Array(b)[0]);
console.log(new Float16Array(new Uint8Array([1, 2]) as any));
