// A runtime kernel that runs ToPrimitive on a value that may be a user
// object INSIDE the runtime; this program's only object-world entry is
// that kernel, so the dispatch judgment must keep the arms (rotation 691
// census).
console.log(new Date({ valueOf() { return 0; } } as any).getTime());
