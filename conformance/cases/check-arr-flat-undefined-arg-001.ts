// S220 — Array.flat(undefined) per ES §23.1.3.10 step 1: `If depth is
// undefined, depthNum = 1`. Equivalent to the 0-arg `xs.flat()`
// default. Fixture uses uniform nesting to avoid pre-existing
// heterogeneous-Array<Array<Any>> L3b carry.
console.log([[1, 2], [3, 4]].flat(undefined));
console.log([[1, 2, 3], [4, 5, 6]].flat(undefined));
console.log([1, 2, 3].flat(undefined));
console.log([[1, 2], [3, 4]].flat());
console.log([["a", "b"], ["c"]].flat(undefined));
console.log([[10]].flat(undefined));
