// S218 — Array.fill(value, undefined [, undefined]) per ES §23.1.3.7
// step 5/9: start=undefined → 0 (default), end=undefined → len.
console.log([1, 2, 3, 4].fill(9, undefined));
console.log([1, 2, 3, 4].fill(9, 1, undefined));
console.log([1, 2, 3, 4].fill(9, undefined, undefined));
console.log([1, 2, 3, 4].fill(9, undefined, 2));
console.log([1, 2, 3, 4].fill(0));
console.log([1, 2, 3, 4].fill(7, 2));
console.log(["a", "b", "c", "d"].fill("x", undefined));
console.log(["a", "b", "c", "d"].fill("y", 1, undefined));
