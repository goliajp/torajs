// The sibling of gen-recv-factory-defaults-001, for the generator
// receivers that one still could not resolve. Same program shape: a
// user class owning the name `next` with its own default evicts the
// bare-name padding table, and every generator whose receiver cannot
// be resolved goes down with it.
//
// Three shapes were missed there:
//
//   1. a generator with a `try` region. Its factory no longer has a
//      single-`return` body by the time padding runs, so inferring
//      the shape answered for plain generators and not for these.
//      The desugar records the pairing itself — read the record.
//   2. the generator class's OWN `return` / `throw`, which re-enter
//      the machine by writing `this.next()`. `this` inside a
//      synthesized method is not a resolvable receiver either, so
//      the compiler's own call was left unpadded.
//   3. a hoisted generator local. A `let` living across a yield
//      becomes a field of the enclosing `__Gen_*`, and the binding's
//      annotation — the only place its class was written — goes with
//      the `let`. `yield* g` builds exactly such a local.

class Cursor {
  next(step: number = 5): number {
    return step;
  }
  return(v: number = 7): number {
    return v;
  }
  throw(v: number = 9): number {
    return v;
  }
}
const c = new Cursor();
console.log(c.next(), c.return(), c.throw());

// 1 + 2 — a try/finally generator, driven through all three methods
const log: string[] = [];
function* guarded(): Generator<any> {
  try {
    yield 1;
    yield 2;
  } finally {
    log.push("cleanup");
  }
}

const g1 = guarded();
console.log(g1.next().value);
const r1 = g1.return("early");
console.log(r1.value, r1.done, log.join(","));
console.log(g1.next().done);

// the same class's throw(), which takes the other injecting path
const g2 = guarded();
console.log(g2.next().value);
try {
  g2.throw(new Error("boom"));
} catch (e: any) {
  console.log("caught", e.message);
}

// a finally that yields — return() answers it before completing
function* yieldingFinally(): Generator<any> {
  try {
    yield "a";
  } finally {
    yield "cleanup";
  }
}
const g3 = yieldingFinally();
console.log(g3.next().value);
const r3 = g3.return("done");
console.log(r3.value, r3.done);

// 3 — yield* through a local, which becomes a generator-state field
function* inner(): number {
  yield 1;
  yield 2;
}
function* delegating(): number {
  const src: any = inner();
  yield* src;
  yield 3;
}
for (const v of delegating()) console.log("delegated", v);

// yield* naming the generator directly keeps the typed lane, and its
// iterator is a state field too
function* direct(): number {
  yield* inner();
  yield 9;
}
for (const v of direct()) console.log("direct", v);
