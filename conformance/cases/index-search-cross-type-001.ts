// §7.2.15 IsStrictlyEqual and §7.2.10 SameValueZero compare by TYPE
// first, so a search element of another type is a legitimate question
// with a legitimate answer: -1, or false. The needle side already had
// this reasoning written down; the ELEMENT side was gated to Number
// and String, which made `[true, false].indexOf(1)` a type error.
const u = [undefined, undefined];
console.log("undef-elem:", u.indexOf(1), u.includes(1), u.lastIndexOf(1));
console.log("undef-same:", u.indexOf(undefined), u.includes(undefined));

const b = [true, false];
console.log("bool-cross:", b.indexOf(1), b.includes(1), b.lastIndexOf(1));
console.log("bool-same:", b.indexOf(false), b.includes(true), b.lastIndexOf(true));

const nl = [null, null];
console.log("null-cross:", nl.indexOf(0), nl.includes(0));
console.log("null-same:", nl.indexOf(null), nl.includes(null));

// The element types that were already admitted keep answering the
// same way, cross-type and same-type alike.
const n = [1, 2, 3];
console.log("num:", n.indexOf("2" as any), n.includes(2), n.lastIndexOf(3));
const s = ["a", "b"];
console.log("str:", s.indexOf(1 as any), s.includes("b"));

// fromIndex still rides along.
console.log("from:", b.indexOf(true, 1), u.indexOf(undefined, 1));
