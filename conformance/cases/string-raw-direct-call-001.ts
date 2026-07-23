// `String.raw(template, ...substitutions)` per ES §22.1.2.4 —
// direct-fn call shape (the tagged-template literal
// `String.raw`...`` surface is a separate parser substrate item).
// Kernel walks template.raw interleaved with substitutions.

// Basic: two parts + one sub.
console.log(String.raw({ raw: ["a", "b"] } as any, 1));
// "a1b"

// Three parts + two subs.
console.log(String.raw({ raw: ["x=", ", y=", "!"] } as any, 10, 20));
// "x=10, y=20!"

// Missing sub (fewer than raw.length-1) → contributes nothing.
console.log(String.raw({ raw: ["a", "b", "c"] } as any, 1));
// "a1bc" (second sub absent)

// Zero substitutions.
console.log(String.raw({ raw: ["only"] } as any));
// "only"

// Empty raw array.
console.log(String.raw({ raw: [] } as any));
// ""

// Sub of various types coerces through ToString.
console.log(String.raw({ raw: ["<", ">"] } as any, true));
// "<true>"

// Sub is null / undefined.
console.log(String.raw({ raw: ["[", "|", "]"] } as any, null, undefined));
// "[null|undefined]"
