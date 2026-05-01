// Adapted from test262 — `console.{error,warn}` route to stderr,
// matching bun / node semantics. The conformance harness compares
// stdout only, so the test verifies that stderr-bound output does
// NOT appear in stdout (and that stdout-bound output is unaffected).
function check(): number {
  console.log("stdout-1");
  console.error("err-1");  // → stderr (NOT in stdout)
  console.log("stdout-2");
  console.warn("warn-1");  // → stderr (NOT in stdout)

  // Various arg types via stderr (just exercises dispatch).
  console.error(42);
  console.error(true);
  console.warn(3.14);
  return 0;
}
console.log(check());
