// `in` is HasProperty (§7.3.12): own face, then the prototype chain.
// Two rows were missing from the chain face while the other faces of
// the same fact answered correctly.

// §24.1.3.10 / §24.2.3.9 — `size` is an accessor on Map.prototype /
// Set.prototype, not a method, so it has no row in the intern table
// the chain walk keys off. `Map.prototype.hasOwnProperty("size")` and
// `getOwnPropertyNames(Map.prototype)` both said yes off the same
// ownership table while `in` said no.
const m: any = new Map<string, number>();
const s: any = new Set<number>();
console.log("size" in m, "size" in s);
console.log(Map.prototype.hasOwnProperty("size"), Set.prototype.hasOwnProperty("size"));
console.log(Object.getOwnPropertyNames(Map.prototype).indexOf("size") >= 0);

// The methods around it were never in doubt — they are intern-table
// names and always walked.
console.log("get" in m, "add" in s, "nope" in m);

// §20.4.3 — a Symbol wrapper hangs off %Symbol.prototype% like the
// three sibling wrappers, but only the siblings had a row in the
// Tag→family table. The read face reaches the family by another
// route, so `.toString()` worked on a cell whose `in` said false.
const w: any = Object(Symbol("d"));
console.log(typeof w, w.toString(), w.description);
console.log("toString" in w, "description" in w, "zz" in w);

// The sibling wrappers, unchanged.
console.log("toFixed" in Object(1), "slice" in Object("a"), "valueOf" in Object(true));

// An own property still shadows nothing away from the chain.
m.size2 = 9;
console.log("size2" in m, "size" in m);
