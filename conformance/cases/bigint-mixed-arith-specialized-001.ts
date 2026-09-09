// §6.1.6.1.x — mixing a BigInt with any other numeric type in an
// arithmetic operator is a TypeError, and the any-lane kernels raise
// the catchable one: `(1n as any) - 2` has always thrown it.
//
// The checker refuses the pair, so an unannotated DIRECT call was the
// one place that answer went missing: a specialization body's
// diagnostics are discarded by design, the pair fell through to a
// static numeric lane, and `f(1n, 2)` answered 0 for `a - b` and
// `[unknown-any-tag]` for `a + b`. A discarded diagnostic is worse
// than no diagnostic — it turns a throw into a wrong number.
//
// The calls are DIRECT on purpose: passing the function as a value
// keeps its parameters on the any lane, where the throw was always
// right, so an indirect call cannot see this.

function sub(a: any, b: any) {
  return a - b;
}
function mul(a: any, b: any) {
  return a * b;
}
function add(a: any, b: any) {
  return a + b;
}
function bor(a: any, b: any) {
  return a | b;
}
function addStr(a: any, b: any) {
  return a + b;
}
function lt(a: any, b: any) {
  return a < b;
}

try {
  console.log("value", sub(1n, 2));
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}
try {
  console.log("value", mul(1n, 2));
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}
try {
  console.log("value", add(1n, 2));
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}
try {
  console.log("value", bor(1n, 2));
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}

// `+` with a String side is CONCAT (§13.15.3), not a mixed-type error
console.log(addStr(1n, "a"), addStr("a", 1n));

// comparison is defined ACROSS the numeric types (§7.2.13)
console.log(lt(1n, 2), lt(3n, 2));
