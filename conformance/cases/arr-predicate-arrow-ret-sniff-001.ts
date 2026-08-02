// the predicate-family "boolean" contextual seed is a fallback, not
// a pin — an arrow whose body returns another type keeps its own
// sniffed return and rides the ToBoolean wedges (§23.1.3).
console.log([0, 1, 2, 3, 4].filter((v) => v % 2));
console.log(["a", "", "b", ""].filter((s) => s));
console.log([1, 2, 3].every((v) => v));
console.log([1, 2, 3].some((v) => v - 1));
console.log([3, 0].find((v) => v));
console.log([1, 2].map((v) => v * 2));
console.log([1, 2].filter((v) => v > 1));
