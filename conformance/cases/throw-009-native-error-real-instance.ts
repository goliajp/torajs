// P7.4-a-2 — a runtime native error (here: assignment to a
// non-writable property, spec §10.1.5 / §10.1.6 → TypeError) is now
// thrown as a REAL Error instance, not a bare message string.
//
// Pre-P7.4 the runtime's torajs_throw_type_error stored only a
// string into the throw slot: `e instanceof TypeError` was false,
// no `.name` / `.message` / `.stack`. Now synthesize_class_globals
// registers each present Error-family class's `__new_<C>` factory
// into a runtime registry (fixed slot + FnAddr); the runtime calls
// the registered factory to build a real, catchable, chain-correct
// instance. The class is injected because `catch (e: TypeError)`
// references it (the referenced-scan now counts catch-param type
// annotations + instanceof RHS).
//
// Verification uses a TYPED catch so `.name` / `.message` / `.stack`
// are typed struct-field accesses (not Any-member access, which is
// the separate A-IDX-1 substrate gap). Message TEXT is intentionally
// not asserted — tora's runtime message differs from V8's exact
// wording (a separate spec-completeness concern, not P7.4 scope);
// only the structural Error-ness is checked, which bun and tora
// agree on.

let a: any = {};
Object.defineProperty(a, "x", {
  value: 1,
  writable: false,
  configurable: true,
  enumerable: true,
});

let reached = false;
try {
  a.x = 2; // assign to non-writable → TypeError
} catch (e: TypeError) {
  console.log(e.name); // TypeError
  console.log(e instanceof TypeError); // true
  console.log(e instanceof Error); // true (prototype chain walk)
  console.log(typeof e.message); // string
  console.log(e.message.length > 0); // true
  console.log(e.stack.startsWith("TypeError")); // true (§20.5.3.4 header)
  reached = true;
}
console.log(reached); // true
console.log(a.x); // 1 (failed assignment left the value unchanged)

// Re-throwing the caught native instance keeps it a real Error:
// catch as the Error parent, instanceof still walks the chain.
let outer = false;
try {
  try {
    a.x = 3;
  } catch (e: TypeError) {
    throw e; // re-throw the real instance
  }
} catch (e: Error) {
  console.log(e instanceof Error); // true
  console.log(e.name); // TypeError (name preserved across re-throw)
  outer = true;
}
console.log(outer); // true
