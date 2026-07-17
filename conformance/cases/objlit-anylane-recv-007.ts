// RFC 20260717-objlit-anylane-recv knife 2f — attached call shapes
// whose callee resolves indirectly bind the HOLDER as `this`
// (§13.3.6 EvaluateCall, the Reference base): getter-as-callee,
// array expando methods, struct-field closure slots. Pre-fix these
// arms fed the raw boxed ABI, so a recv-first callee's __this ate
// argv[0] (a no-arg call read this = undefined and threw).

const o: any = { v: 7, f() { return this.v; } };

// dynobj getter-as-callee: this = the holder
const holder: any = { w: 9, get m() { return o.f; } };
console.log(holder.m()); // undefined (holder has no .v)
const holder2: any = { v: 55, get m() { return o.f; } };
console.log(holder2.m()); // 55

// arr expando method: this = the array
const arr: any = [1, 2];
arr.m = o.f;
console.log(arr.m()); // undefined (arrays have no .v)
arr.v = 33;
console.log(arr.m()); // 33

// arr accessor getter-as-callee
Object.defineProperty(arr, "g", { get() { return o.f; } });
console.log(arr.g()); // 33 (this = arr, arr.v = 33)

// plain (this-free) closures through the same arms stay unchanged
const p: any = { get m() { return () => 42; } };
console.log(p.m()); // 42
console.log("done");
