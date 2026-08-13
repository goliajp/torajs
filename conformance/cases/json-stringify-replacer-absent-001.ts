// The `replacer` slot is not served yet (§25.5.2.1 PropertyList /
// §25.5.2.2 step 3), and a written one is refused rather than
// ignored -- being ignored made it a silent wrong.
//
// This pins the shapes that DO work: slot 2 spelled `null` or
// `undefined` means "no replacer", so the plain serialization and the
// 3-arg `space` form stay byte-identical to bun.

const o = { a: 1, b: { c: 2 } };

console.log(JSON.stringify(o));
console.log(JSON.stringify(o, null));
console.log(JSON.stringify(o, undefined));
console.log(JSON.stringify(o, null, 2));
console.log(JSON.stringify(o, undefined, 2));
console.log(JSON.stringify(o, null, "\t"));
console.log(JSON.stringify([1, [2, [3]]], null, 1));
