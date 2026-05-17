// P4.0 — nested Any-typed dynobj field identity.
// Pre-fix `lower_dynobj_init` + Member-assign match-arm order put
// `_ if v_ty.is_refcounted()` before `Type::Any`. Since Type::Any
// is itself refcounted, that arm grabbed the any-box wrapper ptr
// and stored *that* as the bucket value with tag=ANY_HEAP. Reads
// then returned the wrapper ptr instead of the underlying heap
// object, breaking identity and recursive field access. Fix:
// Type::Any arm moved above is_refcounted; payload (tag, val) is
// extracted from the box at +8/+16 and bucket owns +1 on val via
// __torajs_any_payload_rc_inc when tag == HEAP. Same fix applies
// to box_to_tag_value (closure / fn / array side-table sets).
//
// This bug was the substrate pre-blocker for P4.prototype-chain
// Phase B+C — singleton prototype objects stored in a class's
// dynobj bucket would lose identity, so any chain wiring built
// on top fell apart.

// 1. Object-literal init with nested Any field — identity + readback.
let inner1: any = { x: 1 };
let outer1: any = { p: inner1 };
console.log(outer1.p === inner1);  // true
console.log(outer1.p.x);           // 1

// 2. Member-assign on Any-typed object — same path through ssa_lower.
let inner2: any = { y: 42 };
let outer2: any = {};
outer2.q = inner2;
console.log(outer2.q === inner2);  // true
console.log(outer2.q.y);           // 42

// 3. Array side-table prop set with Any rhs (box_to_tag_value path).
let inner3: any = { z: 99 };
let arr: number[] = [1];
(arr as any).r = inner3;
console.log((arr as any).r === inner3);  // true
console.log((arr as any).r.z);           // 99

// 4. Three-deep nesting — confirms the fix composes.
let a: any = { v: 1 };
let b: any = { p: a };
let c: any = { q: b };
console.log(c.q === b);    // true
console.log(c.q.p === a);  // true
console.log(c.q.p.v);      // 1

// 5. Mixed-tag fields — Any field next to typed-primitive field
//    must not regress.
let mix: any = { p: inner1, n: 7, s: "hello" };
console.log(mix.p === inner1);  // true
console.log(mix.n);             // 7
console.log(mix.s);             // hello
