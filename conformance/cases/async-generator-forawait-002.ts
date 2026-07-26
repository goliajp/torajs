// S2.5 — `for await` over an async iterator the loop does NOT get as
// a direct factory call. Sibling 001 covers `for await (v of ag(n))`,
// which the parser desugars at the AST level because the callee names
// the generator; everything below reaches the generic Stmt::ForOf and
// the SSA iterator-protocol lane instead, where `next()` answers
// Promise<IteratorResult> per §27.6 and each step is awaited
// (ES §14.7.5.10).
//
// The driver is a plain `async function` rather than the usual
// `(async () => {})()`: for-of inside an ARROW function is broken
// independently of this lane (the desugar-minted loop counter is not
// collected as a closure binding, so even `for (v of [1,2])` inside
// `() => {}` is a type error). Tracked separately; sibling 003 is the
// arrow-shaped version once that is fixed.

async function* ag(n: number): AsyncGenerator<number> {
  let i = 0;
  while (i < n) {
    yield i * 2;
    i = i + 1;
  }
}

class Box {
  async *items() {
    yield "a";
    yield "b";
  }
}

function* syncGen(): Generator<number> {
  yield 10;
  yield 20;
}

async function main() {
  // held in a variable — the callee is no longer a factory name
  const held = ag(3);
  for await (const v of held) {
    console.log("held", v);
  }

  // the source is a method call, so the callee is a Member
  const b = new Box();
  const fromMethod = b.items();
  for await (const s of fromMethod) {
    console.log("method", s);
  }

  // an early break still owes the iterator its close (§7.4.9)
  const stopped = ag(9);
  for await (const v of stopped) {
    console.log("stopped", v);
    if (v >= 2) {
      break;
    }
  }

  // a SYNC generator held in a variable keeps the sync next-loop —
  // the await flag must not turn its plain struct step into a promise
  const s = syncGen();
  for (const v of s) {
    console.log("sync", v);
  }

  // and the array lane (each ELEMENT is the promise) is untouched
  for await (const p of [Promise.resolve(7), Promise.resolve(8)]) {
    console.log("arr", p);
  }
}

main();
