// §13.3.6 — CALLING a name a primitive does not have is a runtime
// TypeError, reached after the arguments evaluate; §10.1.8.1 makes
// the read itself `undefined`. Both answers live in the any lane's
// kernels, and `(1 as any).nope()` has always thrown the right one.
//
// The specialization body could not reach them: the terminal-miss
// posture admitted an inferred STRUCT anchor (rotation 620) and left
// every primitive anchor a hard reject. In a specialization body a
// reject is DISCARDED, so what lowering saw was a blank — and the
// call cascade bottomed out at "unsupported member call shape: nope"
// while the optional chain hit "optional chain on receiver type I64
// not yet supported". Both on programs bun runs.

function callMiss(n: any) {
  try {
    n.nope();
    return "no throw";
  } catch (e) {
    return "threw " + (e as any).constructor.name;
  }
}
console.log(callMiss(1), callMiss(true), callMiss("x"));
console.log(callMiss(1.5), callMiss(1n), callMiss({ x: 1 }), callMiss([1]));

// the arguments still evaluate before the throw (§13.3.6 step 4)
function order(n: any) {
  const seen: any[] = [];
  try {
    n.nope(seen.push("arg"));
  } catch (e) {
    seen.push("threw");
  }
  return seen.join(",");
}
console.log(order(1), order(true));

// `?.` on a primitive: never nullish, so the chain proceeds and the
// miss answers undefined
function optMiss(n: any) {
  return n?.nope;
}
console.log(optMiss(1), optMiss(true), optMiss(2.5), optMiss("x"));

function optDeep(n: any) {
  return n?.nope?.deeper;
}
console.log(optDeep(1), optDeep(true));

// the methods that DO exist still dispatch off their own lanes
function real(n: any) {
  return n.toString();
}
console.log(real(1), real(true), real("x"), real(1n));
