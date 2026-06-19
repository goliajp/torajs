// `new Array(n)` pre-fix wrote `n*8` zero bytes into Array<Any>'s
// slot region. 0x00 doesn't decode as a valid boxed AnyValue, so
// `console.log(arr)` printed `[unknown-any-tag, …]` and `arr[i]`
// decoded as null/typeof object — diverging from ES §10.4.2.1
// missing-index semantics (undefined). Fix writes the boxed
// `ANY_UNDEF` immediate per slot (mirrors `__torajs_arr_set_at_any`
// hole-fill at `any.rs:385`), so `arr[i]` reads back as undefined.
// Note: still densely-undefined, not sparse-hole — `console.log(arr)`
// renders `[undefined, …]` not `[ N x empty items ]` because true
// sparse hole needs an elem-kind-tag substrate (L3b).

const a = new Array(3);
console.log(a.length);
console.log(a[0]);
console.log(typeof a[1]);
console.log(a[2] === undefined);

const b = new Array(0);
console.log(b.length);
console.log(b[0]);

const c = new Array(5);
console.log(c.length);
console.log(c[4]);
console.log(typeof c[3]);
console.log(c[0] === undefined && c[4] === undefined);
