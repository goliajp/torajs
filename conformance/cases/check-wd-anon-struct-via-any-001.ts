// W-D anonymous-struct-cell-via-Any IC dispatch — plan-state line
// 1554 narrow follow-up + S126-2 OptChain Any 闭合面。Pre-fix
// `lower_any_member_read` only enumerated `class_name_to_tag`
// (named classes) for the monomorphic class-tag IC dispatch — anon
// ObjectLit stamps live in `anon_stamp_pool` (W-J A1 follow-up
// `cc6416a6`) and silently fell through to the dynobj path, which
// returned ANY_UNDEF for a struct-cell receiver.
//
// User-visible:
//   const da: any = { x: 1, y: 2 };
//   da.x        // pre-fix undefined; post-fix 1
//   da?.x       // pre-fix undefined (OptChain Any went through the
//               // same Any.Member substrate); post-fix 1
//
// Symmetry with named-class W-D (RFC `any-class-member-read` —
// plan-state line 1507) — same monomorphic IC arm now treats
// anon-stamped sids as full class layouts.

const da: any = { x: 1, y: 2 };
console.log(da.x);
console.log(da.y);

// OptChain Any (S126-2) shares the Any.Member substrate, so the
// anon stamp also unblocks `da?.field`.
console.log(da?.x);
console.log(da?.y);
console.log(da?.missing);

// Multi-field shape — different sid, fresh IC arm.
const eb: any = { a: "hello", b: 7.5, c: true };
console.log(eb.a);
console.log(eb.b);
console.log(eb.c);
console.log(eb?.a);
console.log(eb?.missing);

// Empty anon ObjectLit — no field, all `.x` reads return undefined
// via the dynobj fallback (no IC candidate matches).
const empty: any = {};
console.log(empty.x);
console.log(empty?.x);
