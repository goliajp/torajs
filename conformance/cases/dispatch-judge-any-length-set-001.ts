// A runtime kernel that runs ToPrimitive on a value that may be a user
// object INSIDE the runtime; this program's only object-world entry is
// that kernel, so the dispatch judgment must keep the arms (rotation 691
// census).
const a: any = [1, 2, 3];
a.length = { valueOf() { return 1; } };
console.log(a.length);
