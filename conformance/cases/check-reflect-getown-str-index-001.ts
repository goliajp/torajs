// W-M-rest — Object.getOwnPropertyDescriptor(str, "<idx>") returns
// per-character data descriptor. Spec ES §22.1.5.2:
//   { value: char_at(idx), writable: false, enumerable: true,
//     configurable: false }
// Note enumerable=true (unlike `length`'s all-false flags). Out-of-
// range / negative / non-canonical keys return undefined.
//
// 6 shapes: "hello"[0]/[4] in-range / [5] OOB / [-1] negative /
// "" empty / "x"[0] single-char. Bun parity verified byte-equal.

const d1 = Object.getOwnPropertyDescriptor("hello", "0");
console.log((d1 as any).value);
console.log((d1 as any).writable);
console.log((d1 as any).enumerable);
console.log((d1 as any).configurable);

const d2 = Object.getOwnPropertyDescriptor("hello", "4");
console.log((d2 as any).value);

console.log(Object.getOwnPropertyDescriptor("hello", "5"));
console.log(Object.getOwnPropertyDescriptor("hello", "-1"));
console.log(Object.getOwnPropertyDescriptor("", "0"));

const d6 = Object.getOwnPropertyDescriptor("x", "0");
console.log((d6 as any).value);
