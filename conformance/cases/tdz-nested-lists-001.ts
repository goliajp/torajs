// §8.1.1.1 step 4 — 603 settled the one shape a compiler can decide
// without tracking initialization at run time: a read written EARLIER
// in a statement list than the `let` / `const` that binds it. It ran
// that rule on the top-level list only.
//
// The reasoning is about a LIST, not about the top level. Every list
// is linear the same way, so a body's own `const` is in dead zone for
// a read written earlier in that same body, whenever the body runs.
// Before this, those read a name nothing declared and the compiler
// said so loudly — `unknown identifier a`, at compile time, for a
// program that should throw a catchable ReferenceError.

function inBlock(): void {
  try {
    {
      console.log(a);
      const a = 5;
    }
  } catch (e: any) {
    console.log("block", e.constructor === ReferenceError);
  }
}
inBlock();

function inBody(): void {
  console.log(a);
  const a = 5;
}
try {
  inBody();
} catch (e: any) {
  console.log("body", e.constructor === ReferenceError);
}

function inBodyLet(): void {
  console.log(b);
  let b = 5;
}
try {
  inBodyLet();
} catch (e: any) {
  console.log("body-let", e.constructor === ReferenceError);
}

const inFnExpr = function (): void {
  console.log(c);
  const c = 5;
};
try {
  inFnExpr();
} catch (e: any) {
  console.log("fn-expr", e.constructor === ReferenceError);
}

function inTry(): void {
  try {
    console.log(d);
    const d = 5;
  } catch (e: any) {
    console.log("try-body", e.constructor === ReferenceError);
  }
}
inTry();

// Nothing crosses a list boundary. A function reading an outer
// binding is the CALLED shape — legal whenever the call happens after
// the declaration has run, which is the normal case and must stay
// unmarked.
function readsOuter(): number {
  return outer;
}
const outer = 7;
console.log("outer", readsOuter());

// An inner declaration of the same name is the one being read.
function shadows(): void {
  const e = 1;
  {
    const e = 2;
    console.log("shadow", e);
  }
  console.log("shadow-outer", e);
}
shadows();

// Ordinary order in a body is untouched.
function ordinary(): void {
  const f = 3;
  console.log("ordinary", f);
}
ordinary();
