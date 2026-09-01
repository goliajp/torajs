// ShortStr values through the pair-read APIs (fromEntries pair
// arrays, gOPD data slots) keep value semantics — the slot
// normalizes to a heap Str on first pair read and every consumer
// borrows a plain cell (546-02 Class B batch).
const o = Object.fromEntries([["k", "ab"], ["m", 42]] as any);
console.log(JSON.stringify(o));
const xs: any = ["ab", 7];
const d0 = Object.getOwnPropertyDescriptor(xs, 0);
const d1 = Object.getOwnPropertyDescriptor(xs, 1);
console.log(d0.value, d0.writable, d1.value);
// The slot still reads correctly after the normalize (borrow lanes
// and the pair lane agree on the same heap cell).
console.log(xs[0], xs.indexOf("ab"));
