// §23.2.5.1 steps 5.b-5.d — a typed array built from an object.
// Three sources with three different spec operations behind them.

// §23.2.5.1.4 — an array (which is an iterable).
console.log(new Uint8Array([1, 2, 300]));
console.log(new Int8Array([1, -1, 128]));
console.log(new Float64Array([0.5, 1.5]));
console.log(new Uint8ClampedArray([1.5, 2.5, -3, 400]));
console.log(new Uint8Array([]).length);

// §23.2.5.1.3 — an array-like: `length` decides, and a hole reads
// as `undefined`, which the element coercion turns into 0 (or NaN
// for a float).
console.log(new Uint8Array({ length: 3, 0: 7, 1: 8, 2: 9 } as any));
console.log(new Uint8Array({ length: 2 } as any));
console.log(new Float64Array({ length: 2 } as any));
console.log(new Uint8Array({} as any).length);

// §23.2.5.1.4 — anything with @@iterator.
console.log(new Uint8Array(new Set([1, 2, 3]) as any));
function* gen() { yield 4; yield 5; }
console.log(new Uint8Array(gen() as any));
console.log(new Uint8Array(new Map([[1, 2]]).keys() as any));

// §23.2.5.1.2 — another typed array converts element by element,
// so a value that does not fit the destination wraps exactly as a
// direct write would.
const src = new Uint16Array([1, 2, 300]);
console.log(new Uint8Array(src), new Int32Array(src), new Float64Array(src));
console.log(new Uint8Array(src).buffer === src.buffer);

// The content types have to agree — this is the one place a typed
// array refuses to coerce rather than converting.
const bi = new BigInt64Array([1n, -2n]);
console.log(bi, new BigUint64Array(bi));
try { new BigInt64Array(new Uint8Array(2) as any); } catch (e) { console.log((e as Error).constructor.name); }
try { new Uint8Array(bi as any); } catch (e) { console.log((e as Error).constructor.name); }
try { new Uint8Array([1n] as any); } catch (e) { console.log((e as Error).constructor.name); }
try { new BigInt64Array([1] as any); } catch (e) { console.log((e as Error).constructor.name); }

// A STRING is a primitive even though it lives on the heap, so it
// takes ToIndex and not the object path.
console.log(new Uint8Array("ab" as any));
console.log(new Uint8Array("3" as any));
console.log(new Uint8Array(true as any).length);

// A view over a buffer still takes the buffer path, not this one.
const b = new ArrayBuffer(4);
console.log(new Uint8Array(b).length, new Uint8Array(b).buffer === b);
