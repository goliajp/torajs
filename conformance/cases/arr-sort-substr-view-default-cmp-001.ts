// Default `sort()` on a split product must compare the VIEWS' text.
//
// `"c b a".split(" ")` answers an array whose slots are 32-byte
// substring views into the parent string. The default-comparator lane
// handed each pair to the Str compare kernel, which read a view by the
// owned-string layout — comparing the parent pointer and offset as if
// they were text — and left the array in its original order: this
// program printed `cba`. Pre-existing on every spelling (chained,
// `let`, `const`); found while probing the rotation-467 split-product
// write face. The kernel now reads each operand by the cell's own flags.

console.log("c b a".split(" ").sort().join(""));

let a = "pear fig apple date".split(" ");
a.sort();
console.log(a.join(","));

// heap parent (not a string literal): same views, same compare
const src = ("zeta beta alpha gamma " + "delta").trim();
const h = src.split(" ");
h.sort();
console.log(h.join(" "), h.length);

// non-ASCII parent: views read through the parent's UTF-16 stride
const u = "世 ab 界 a".split(" ");
u.sort();
console.log(u.join("|"));

// mixed empty pieces sort first; a user comparator is unaffected
const e = ",b,,a".split(",");
e.sort();
console.log(JSON.stringify(e));
const c = "c b a".split(" ");
c.sort((x, y) => (x < y ? 1 : x > y ? -1 : 0));
console.log(c.join(""));
