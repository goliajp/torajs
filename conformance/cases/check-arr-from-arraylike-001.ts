// Chunk 696 — Array.from array-like `{length: n}` source (ES
// §23.1.2.1 non-iterable branch): the src materializes as the
// same dense undefined-filled Array<Any> `new Array(n)` mints
// (every index read answers undefined; mapFn sees (undefined, i)).
// Probe also flushed a pre-existing silent-wrong fixed here: an
// Any-ret mapFn's dst array was minted with the typed alloc, so
// the block never self-described as FLAG_ARR_ANY and kind-aware
// index reads answered undefined / NaN (chunk 625 fixed the same
// bug in flat_map).
console.log(Array.from({ length: 3 }, (_, i) => i * 2));
console.log(Array.from({ length: 3 }));
const o = { length: 2 };
console.log(Array.from(o, (_, i) => i + 10));
console.log(Array.from({ length: 0 }, (_, i) => i));
// mapFn-result index read + arithmetic (the silent-wrong shape:
// two-param mapFn infers an any sig, dst must self-describe)
const r = Array.from({ length: 4 }, (_, i) => i);
console.log(r[3]);
let n = 0;
n += r[3];
console.log(n);
// existing Arr source with two-param mapFn (same dst face)
const a = Array.from([10, 20, 30, 40], (x, i) => x + i);
console.log(a[3]);
// single-param mapFn regression (typed dst lane unchanged)
console.log(Array.from([5, 6, 7], (x) => x * 2));
