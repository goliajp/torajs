// r380 — the returned-literal twin of objlit-method-detached-001.
// `makeCounter().read` is the same detached method, but the receiver
// is a call result rather than a binding, so 001's annotation widen
// has nothing to write on. The literal in the `return` is wrapped
// `as any` instead — measured as the only forcing that works
// (widening the fn's return type alone still SIGSEGVs: it moves what
// the call site sees while the literal keeps lowering nominal).

function mk() {
  return {
    n: 3,
    read() { return this.n; },
    bump() { this.n++; return this.n; },
  };
}

// direct calls off the result stay right
console.log(mk().read());

// so do receiver writes through a bound result
const o = mk();
o.bump();
console.log(o.read());

// the detached read is what used to crash
const t = mk().read;
try { t(); } catch (err) { console.log("bare:", (err as Error).constructor.name); }

// and an explicit thisArg reaches the body
console.log(t.call({ n: 7 }));

// a second fn returning its own literal keeps its own forcing
function other() {
  return { label: "x", get() { return this.label; } };
}
const g = other().get;
console.log(g.call({ label: "hit" }));
console.log(other().get());

// a fn whose returned literal is never read as a value stays nominal
function plain() {
  return { v: 1, show() { return this.v; } };
}
console.log(plain().show());
