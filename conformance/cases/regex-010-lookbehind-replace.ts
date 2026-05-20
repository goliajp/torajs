// Phase 1c.4.b: lookbehind on the heavier surface methods (replace /
// match-all / find-pos) to exercise the assertion path through
// repeated probe positions in the outer search loop.

// String.replace with positive lookbehind
console.log("foobar".replace(/(?<=foo)bar/, "BAR"));
console.log("xxxbar".replace(/(?<=foo)bar/, "BAR"));

// String.replace with negative lookbehind
console.log("xxxbar foobar".replace(/(?<!foo)bar/, "BAR"));

// String.replaceAll exercises lookbehind at multiple positions
console.log("foobar barbar".replaceAll(/(?<=foo)bar/g, "BAR"));
console.log("a1 b2 c3".replaceAll(/(?<=[a-z])\d/g, "X"));

// String.match with capture group + lookbehind context
const m = "foo123 bar456".match(/(?<=[a-z]+)(\d+)/);
console.log(m === null ? "null" : m[0]);

// .test() smoke for negative lookbehind with anchor
console.log(/(?<!^)abc/.test("abc"));
console.log(/(?<!^)abc/.test("xabc"));

// Lookbehind at end-of-string boundary
console.log(/(?<=end)$/.test("the end"));
console.log(/(?<=end)$/.test("the mid"));
