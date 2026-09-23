// Every callback-taking Array method, each with an UNANNOTATED arrow,
// so each one exercises the seed table that gives a lifted closure its
// parameter and return annotations.
//
// This exists because `toSorted` was simply absent from that table —
// its comparator got no parameter types at all and the call failed one
// step later, at the argument check, with a message that named the
// argument rather than the table. A method missing from a per-method
// table is invisible until someone writes the shape, so the shapes are
// written here, all of them, in one place.
//
// The survey that produced this found no other gap; that result is
// worth keeping true.
const ns = [1, 2, 3];

console.log("map", ns.map((x) => x * 2).join(","));
console.log("map-idx", ns.map((x, i) => x + i).join(","));
console.log("filter", ns.filter((x) => x > 1).join(","));
console.log("filter-idx", ns.filter((x, i) => i > 0).join(","));
ns.forEach((x) => console.log("forEach", x));
console.log("find", ns.find((x) => x > 1));
console.log("findLast", ns.findLast((x) => x > 1));
console.log("findIndex", ns.findIndex((x) => x > 1));
console.log("findLastIndex", ns.findLastIndex((x) => x > 1));
console.log("some", ns.some((x) => x > 2));
console.log("every", ns.every((x) => x > 0));
console.log("flatMap", ns.flatMap((x) => [x, x * 2]).join(","));
console.log("reduce", ns.reduce((a, b) => a + b, 0));
console.log("reduceRight", ns.reduceRight((a, b) => a + b, 0));

// the comparator pair — same signature, and `toSorted` does not mutate
const us = [3, 1, 2];
console.log("sort", us.sort((a, b) => a - b).join(","));
const vs = [3, 1, 2];
console.log("toSorted", vs.toSorted((a, b) => a - b).join(","), vs.join(","));

// strings through the same comparator slot
const ws = ["b", "a", "c"];
console.log("toSorted-str", ws.toSorted((a, b) => a < b ? -1 : a > b ? 1 : 0).join(","));

// no comparator at all — the default is lexicographic, not the seed
console.log("sort-default", [10, 9, 1].sort().join(","));
console.log("toSorted-default", [10, 9, 1].toSorted().join(","));

// Array.from's mapper is the same kind of slot on a static
console.log("from-map", Array.from([1, 2], (x) => x * 2).join(","));
