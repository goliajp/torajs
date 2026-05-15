// T-27.b — Function as Object (FnSig form). Top-level FnDecls and
// non-capturing function expressions become Type::FnSig at the SSA
// layer (raw fn pointer, no env block) so the in-closure
// CLOSURE_PROPS_OFF storage from T-27.a doesn't apply. Instead we
// keep a global side table keyed by fn pointer → dynobj
// (`__torajs_fnprops_set` / `_get_tag` / `_get_value`).
//
// Top-level FnDecls live for the entire program — no drop hook
// needed. Hash 256 buckets with MurmurHash-style finalizer mix.
// Lazy alloc on first prop access; fns that never get `.x = v` pay
// zero cost (no node, no dynobj).

function f() { return 1; }

// First write — interns the bucket node + allocs dynobj.
f.x = 7;
console.log(f.x);             // 7
console.log(typeof f.x);      // number

// Multiple properties — same fn → same dynobj.
f.name2 = "alice";
f.flag = true;
console.log(f.name2);         // alice
console.log(f.flag);          // true

// Overwrite same key — dynobj_set replaces in-place.
f.x = 99;
console.log(f.x);             // 99

// Read of unset — fnprops_get_tag returns ANY_UNDEF.
console.log(typeof f.unset);  // undefined

// Function still callable.
console.log(f());             // 1

// Non-capturing function expression → also FnSig path.
let g = function() { return 2; };
g.tag = "hi";
console.log(g.tag);           // hi
console.log(g());             // 2
