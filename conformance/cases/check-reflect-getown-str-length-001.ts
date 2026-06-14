// W-M — Object.getOwnPropertyDescriptor(str, "length") returns 4-key data descriptor
// Spec ES §22.1.5.1: String's `length` own property is `{value, writable: false,
// enumerable: false, configurable: false}` — every flag false, unlike Array's
// writable length. Static fast path via __torajs_anyv_str_length_descriptor.
// 3 shapes: multi-char "hello" / empty "" / single "x". Bun parity verified
// byte-equal. `(desc as any).field` cast avoids dynobj pretty-print fallback.

const d1 = Object.getOwnPropertyDescriptor("hello", "length");
console.log((d1 as any).value);
console.log((d1 as any).writable);
console.log((d1 as any).enumerable);
console.log((d1 as any).configurable);

const d2 = Object.getOwnPropertyDescriptor("", "length");
console.log((d2 as any).value);

const d3 = Object.getOwnPropertyDescriptor("x", "length");
console.log((d3 as any).value);
