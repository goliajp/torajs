// T-29 — Array-as-Object. Per ECMAScript Array values are first-class
// Objects: `arr.x = v` and `arr.x` are spec-required (used by RegExp
// match-result attributes like `.input` / `.index`, by iterator
// polyfills attaching state, by `Array.from`-with-cb that stamps
// metadata on the result). Pre-T-29 tora rejected with "field
// assignment target must be a struct, got Array(...)".
//
// Implementation: side table keyed by array ptr → dynobj
// (__torajs_arrprops_*), mirroring the T-27.b FnSig fnprops table.
// Distinct from FnSig because arrays DO drop — arr_drop and
// arr_drop_any get a `__torajs_arrprops_drop_entry(arr)` hook before
// the actual free, walking the bucket chain to drop the dynobj +
// remove the node.
//
// Layout-extension (mirroring T-27.a CLOSURE_PROPS_OFF for Closure)
// would put a props slot in the Array header directly, but Array is
// a much higher-traffic type — every access derives from
// ARR_DATA_OFF, which would shift across ~28 sites. Side-table
// contains the change to runtime_str.c + ssa_lower's Member-on-Array
// branch + arr_drop's hook call.

let arr = [1, 2, 3];

// First write: side-table allocs the dynobj.
arr.x = "hello";
console.log(arr.x);              // hello
console.log(typeof arr.x);       // string

// Existing array surface unaffected.
console.log(arr.length);         // 3
console.log(arr[0]);             // 1
console.log(arr[2]);             // 3

// Multiple properties — same dynobj.
arr.tag = 42;
arr.flag = true;
console.log(arr.tag);            // 42
console.log(arr.flag);           // true

// Overwrite same key.
arr.x = 99;
console.log(arr.x);              // 99

// Read of unset prop → undefined.
console.log(typeof arr.unset);   // undefined

// Existing methods still work (not shadowed).
arr.push(4);
console.log(arr.length);         // 4
console.log(arr[3]);             // 4
