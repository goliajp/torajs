// S217 — Array.{indexOf,lastIndexOf,includes}(needle, undefined) per
// ES §22.1.3.{13,14,16}: ToIntegerOrInfinity(undefined)=0. Note that
// explicit-undefined behaves like fromIndex=0, distinct from Array's
// omitted-fromIndex default for lastIndexOf (which is len-1).
console.log([1, 2, 1, 3].indexOf(1, undefined));
console.log([1, 2, 1, 3].indexOf(2, undefined));
console.log([1, 2, 1, 3].indexOf(9, undefined));
console.log([1, 2, 1, 3].lastIndexOf(1, undefined));
console.log([1, 2, 1, 3].lastIndexOf(3, undefined));
console.log([1, 2, 1, 3].lastIndexOf(9, undefined));
console.log([1, 2, 3].includes(1, undefined));
console.log([1, 2, 3].includes(2, undefined));
console.log([1, 2, 3].includes(9, undefined));
console.log(["a", "b", "c"].indexOf("b", undefined));
console.log(["a", "b", "c"].includes("a", undefined));
