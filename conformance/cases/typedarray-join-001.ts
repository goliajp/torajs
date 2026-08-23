// §23.2.3.16 `%TypedArray%.prototype.join`.
//
// The same six steps as §23.1.3.15: separator once, then each
// element, `undefined` rendering as the empty string. The elements
// of a typed array are always ASCII — Numbers or BigInts — so the
// separator is the only piece carrying an encoding, which is
// exactly why this delegates to the array join rather than growing
// a second copy of that walk.

const a = new Uint8Array([1, 2, 3]);

// default separator, explicit separator, empty separator
console.log(a.join());
console.log(a.join("-"));
console.log(a.join(""));
console.log(a.join(", "));

// an undefined separator is the default; other values are ToString'd
console.log(a.join(undefined));
console.log(a.join(0 as any));
console.log(a.join(null as any));
console.log(a.join(true as any));
console.log(a.join({ toString: () => "|" } as any));

// a non-ASCII separator — the piece that carries an encoding
console.log(a.join("→"));
console.log(a.join("日本"));
console.log(new Uint8Array([7]).join("→"));

// empty and single views
console.log(JSON.stringify(new Uint8Array(0).join()));
console.log(new Uint8Array([9]).join("-"));

// element rendering goes through the element type
console.log(new Int8Array([-1, 0, 127]).join());
console.log(new Uint8ClampedArray([0, 128, 255]).join());
console.log(new Float64Array([1.5, -0, NaN, Infinity, -Infinity]).join());
console.log(new Float32Array([0.1, 0.5]).join());
console.log(new BigInt64Array([-1n, 0n, 9007199254740993n]).join());
console.log(new BigUint64Array([0n, 18446744073709551615n]).join("/"));

// §23.2.3.31 toString IS join() with no separator, and it is what
// String(), template interpolation and `+ ""` all reach through
// ToPrimitive — one dispatch arm, four spellings.
const t = new Uint8Array([4, 5, 6]);
console.log(String(t));
console.log(`${t}`);
console.log(t + "");
console.log([1, 2].join("") === new Uint8Array([1, 2]).join(""));

// a view onto part of a buffer joins only its own elements
const buf = new ArrayBuffer(8);
const full = new Uint8Array(buf);
full.set([1, 2, 3, 4, 5, 6, 7, 8]);
console.log(full.subarray(2, 5).join("-"));
console.log(new Uint16Array(buf, 2, 2).join("-"));

// a length-tracking view over a resizable buffer joins what is
// there at the time
const rab = new ArrayBuffer(4, { maxByteLength: 8 });
const track = new Uint8Array(rab);
track.set([1, 2, 3, 4]);
console.log(track.join("-"));
rab.resize(6);
console.log(track.join("-"));
rab.resize(2);
console.log(track.join("-"));
