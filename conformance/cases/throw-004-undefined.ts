// P7.2a — `throw undefined`. Pre-fix the throw typecheck rejected it
// ("throw value must be 8-byte-shaped, got Undefined"); even past the
// typecheck, undefined and null both collapse to ConstPtrNull at the
// SSA layer, so the throw match mis-tagged undefined as ANY_NULL=0.
// Fix: check.rs accepts Type::Undefined; the throw lowering consults
// the frontend expr-type (same idiom as lower_to_tag_value) and
// emits ANY_UNDEF=5 / payload 0 so a `catch (e: any)` rebuilds a
// real undefined — distinct from null.
//
// Verification uses `typeof` (the spec-observable difference:
// `typeof undefined === "undefined"` vs `typeof null === "object"`,
// §6.1) plus direct print. `e === undefined` is intentionally NOT
// used — strict-eq on a `: any` binding is a separate typecheck gap
// unrelated to throw; typeof exercises the ANY_UNDEF≠ANY_NULL tag
// distinction more directly anyway.

// 1. Direct throw + catch as any: undefined stays undefined.
try {
  throw undefined;
} catch (e: any) {
  console.log(typeof e); // undefined
  console.log(e);        // undefined
}

// 2. null is NOT collapsed into undefined (the tag distinction).
try {
  throw null;
} catch (e: any) {
  console.log(typeof e); // object
  console.log(e);        // null
}

// 3. Thrown across a fn boundary.
function boom(): void {
  throw undefined;
}
try {
  boom();
} catch (e: any) {
  console.log(typeof e); // undefined
}

// 4. finally runs, then outer catch still sees undefined.
try {
  try {
    throw undefined;
  } finally {
    console.log("finally"); // finally
  }
} catch (e: any) {
  console.log(typeof e); // undefined
}
