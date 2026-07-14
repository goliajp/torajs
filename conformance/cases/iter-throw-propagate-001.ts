// An iterator step that THROWS forwards the abrupt completion — ES
// §7.4.6 IteratorNext returns if abrupt, and never looks at a result.
//
// Every lane that drives the iterator protocol invokes user code
// (`[Symbol.iterator]()`, `next()` — a generator body runs inside the
// call), and an aborted fn answers the throw sentinel, not a value.
// Reading `.done` / `.value` off that sentinel is a wild deref: the
// typed for-of lane SIGSEGV'd on a generator that throws on its first
// step, and the runtime kernel's lanes silently clobbered the thrown
// Error (`e.message` came back empty).

function* mk(): Generator<number> {
  throw new Error("boom");
}

// Direct next() — no protocol lane involved, the baseline.
try {
  const it = mk();
  it.next();
} catch (e) {
  console.log("direct next:", e.message);
}

// Typed for-of over a captured generator — the SSA protocol lane.
try {
  const it = mk();
  for (const v of it) {
    console.log(v);
  }
} catch (e) {
  console.log("typed for-of:", e.message);
}

// The same behind `any` — the runtime iteration kernel's class lane.
try {
  const it: any = mk();
  for (const v of it) {
    console.log(v);
  }
} catch (e) {
  console.log("any for-of:", e.message);
}

// Spread — the kernel's other driver.
try {
  const it: any = mk();
  const drained = [...it];
  console.log(drained.length);
} catch (e) {
  console.log("spread:", e.message);
}

// Destructuring — bounded walk and rest drain both forward the throw,
// and the iterator is NOT resumed afterwards.
let steps = 0;
function* counted(): Generator<number> {
  steps = steps + 1;
  throw new Error("boom");
}
try {
  const [a, b] = counted();
  console.log("no throw", a, b);
} catch (e) {
  console.log("dstr:", e.message);
}
try {
  const [...rest] = counted();
  console.log("no throw", rest.length);
} catch (e) {
  console.log("dstr rest:", e.message);
}
console.log("steps", steps);
