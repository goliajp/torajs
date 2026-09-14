// num_width — a receiver-first callee's FIRST USER param takes the
// element / accumulator / settled-value width. `user_params` skipped
// the lifted `__env` but not the hidden `__this`, so the element
// class was wired onto the receiver slot and the real parameter kept
// its default (integer) width: a fractional array's callback saw
// truncated values in every receiver-first position (rotation 649,
// found while auditing the RFC 20260905 residue-1 lanes).
function C3(this: any, v: number): boolean { console.log("every", typeof this, v); return v > 0; }
const o = { k: 1 };
console.log([2.5, 0.5].every(C3, o));
console.log([2.5, 0.5].some(C3, o));
console.log([2.5, 0.5].map(function (this: any, v: number): number { console.log("map", typeof this, v); return v * 2; }, o).join(","));
console.log([2.5, 0.5].filter(function (this: any, v: number): boolean { return v > 1; }, o).join(","));
[2.5, 0.5].forEach(function (this: any, v: number): void { console.log("each", v + (this as any).k); }, o);
console.log([1.5, 2.5].reduce(function (this: any, acc: number, x: number): number { return acc + x; }, 0.25));
