// S233 — `String.split(undefined)` per ES §22.1.3.21 step 2-3:
// separator===undefined skips the splitting and returns `[S]`
// (a single-element array containing the receiver). Pre-fix tr
// fell through to the str_split helper with a ConstPtrNull sep
// slot which SIGSEGV'd the (Str, Str) ABI. The ssa_lower_str
// S233 reroute sends 1-arg-undef calls to str_split_no_sep with
// only the receiver, matching the 0-arg shape.
console.log("a,b,c".split(undefined));
console.log("hello".split(undefined));
console.log("".split(undefined));
console.log("a,b,c".split(undefined).length);
