// W-L — Object.getOwnPropertyDescriptor(arr, "length") returns 4-key data descriptor
// RFC C5a static fast path (reflect.rs:178 / ssa_lower.rs:15699-15729):
//   { value: arr.length, writable: true, enumerable: false, configurable: false }
// 3 shapes: multi-elem [1,2,3] / empty [] / single [42]. Bun parity verified byte-equal.
// `(desc as any).field` cast avoids dynobj pretty-print fallback ([object] placeholder)
// — print full desc requires independent print_anyv dynobj pretty-print path.

const d1 = Object.getOwnPropertyDescriptor([1, 2, 3], "length");
console.log((d1 as any).value);
console.log((d1 as any).writable);
console.log((d1 as any).enumerable);
console.log((d1 as any).configurable);

const d2 = Object.getOwnPropertyDescriptor([], "length");
console.log((d2 as any).value);

const d3 = Object.getOwnPropertyDescriptor([42], "length");
console.log((d3 as any).value);
