// RC-4 arguments-object companion — a throw inside an IIFE nested in
// a named fn must propagate to the caller's caller. The M4.3.b
// may-throw analysis only recorded Ident callees, so the IIFE call
// fell out of the fixed-point: the outer fn was never marked
// may_throw, its caller's emit_throw_check was skipped, and the
// throw was silently swallowed (the program printed the post-call
// line and exited 0).

// IIFE throw inside a named fn
function t1(): void {
  (function () {
    throw new Error("iife-inner");
  })();
  console.log("unreachable-1");
}
try {
  t1();
} catch (e) {
  console.log("caught-1");
}

// chained-call throw inside a named fn
function mk(): () => void {
  return function () {
    throw new Error("chained-inner");
  };
}
function t2(): void {
  mk()();
  console.log("unreachable-2");
}
try {
  t2();
} catch (e) {
  console.log("caught-2");
}

// two named-fn frames above the IIFE
function inner3(): void {
  (function () {
    throw new Error("deep-inner");
  })();
}
function t3(): void {
  inner3();
  console.log("unreachable-3");
}
try {
  t3();
} catch (e) {
  console.log("caught-3");
}

console.log("done");
