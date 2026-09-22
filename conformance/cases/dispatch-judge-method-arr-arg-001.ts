// A runtime kernel that runs ToPrimitive on a value that may be a user
// object INSIDE the runtime; this program's only object-world entry is
// that kernel, so the dispatch judgment must keep the arms (rotation 691
// census).
// Here the object is an ARRAY argument of an any-method call: the
// method kernel coerces it, and its ToString enters the arr arm.
const s: any = "ab";
console.log(s.padStart(5, [7, 8]));
