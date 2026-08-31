// Rotation 543 — the first item in an array literal decided the
// element type for all of them, and nothing ever asked the rest
// whether they agreed. Everything below was wrong before this, in
// three different ways:
//
//   [...[1, 2], ...["a"]]   printed 4309125696 — a Str pointer read
//                           through an I64 slot
//   [...[1, 2], ..."ab"]    printed 2.1403223537e-314 — the same
//                           pointer read as an f64
//   [...[1, 2], "a"]        printed a pointer, and it has only ONE
//                           spread in it
//   [...["a"], ...[1, 2]]   exit 139 — the reverse order reads a
//                           small integer as a pointer
//   [..."ab", 1]            exit 139, three tokens long
//   [...new Set([1, 2]), ...["a"]]
//                           a loud reject citing a follow-up
//
// When the items disagree the literal is an `Array<Any>`, which is
// what the spelling means and what the any assembler builds. Numbers
// stay one bucket on purpose: `[...[1, 2], ...[3.5]]` has always
// worked because `num_width` widens the anon slots together, and
// routing it through the any lane would tax a path that is fast
// precisely because it is not boxed.
//
// The reject was the stale half. `__torajs_arr_extend_any` already
// reads the source header's element kind and routes a typed block
// through `__torajs_arr_extend_typed_into_any`, which boxes per slot,
// honours Bool's one-byte stride, rc_incs each heap cell and
// materializes an inline Substr view. The follow-up it deferred to
// had landed; nobody came back for the reject.
console.log([...[1, 2], ...["a"]]);
console.log([...["a"], ...[1, 2]]);
console.log([..."ab", ...[1, 2]]);
console.log([...[1, 2], ..."ab"]);
console.log([...[1, 2], ...[true]]);
console.log([...[true, false], ...["x"]]);

console.log([...[1, 2], "a"]);
console.log([..."ab", 1]);
console.log([1, ...["a"]]);
console.log(["a", ...[1, 2]]);

const a = [1, 2];
const b = ["x"];
console.log([...a, ...b], a, b);

console.log([...[[1]], ...[["a"]]]);
console.log([...[1, 2], ...[[3]]]);

console.log([...new Set([1, 2]), ...["a"]]);
console.log([...[1, 2], ...new Set(["a"])]);

const m = new Map([[1, "a"]]);
console.log([...m, ...[1]]);

const xs: any[] = [1, "a"];
console.log([...xs, 2], xs);

console.log([..."👋a", 1]);

// numbers stay one bucket — unchanged, and must stay that way
console.log([...[1, 2], ...[3.5]]);
console.log([...[3.5], ...[1, 2]]);
