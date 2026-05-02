// Phase L.2 — `async function` + `await` MVP. Parser sets a flag on
// the FnDecl (via Ast.async_fns) and rewrites `await e` to `e.value`
// at parse time. desugar_async wraps the body's tail return in a
// Promise:
//
//   async function f(args): T { ... return e; }
//   ⇓
//   function f(args): Promise<T> {
//     let __async_p = new Promise(<default T>);
//     ...
//     __async_p.do_resolve(e);
//     return __async_p;
//   }
//
// L.2 MVP scope:
//   - `await e` is `e.value` — synchronous read of an already-fulfilled
//     Promise. Real spec semantics (microtask queue + then-callback
//     resume) deferred to L.3.
//   - async fns must have a single tail `return` — multi-branch
//     returns trigger a tr ownership tracker bug (silent wrong output)
//     so desugar_async panics with a helpful error in that case.
//   - The user-declared `Promise<T>` class from L.1 is what wraps the
//     return; `Promise` must be the L.1 shape (with `do_resolve`).

class Promise<T> {
  state: number;
  value: T;
  constructor(seed: T) {
    this.state = 0;
    this.value = seed;
  }
  do_resolve(v: T): void {
    if (this.state === 0) {
      this.state = 1;
      this.value = v;
    }
  }
}

function presolved(v: number): Promise<number> {
  let p = new Promise(v);
  p.do_resolve(v);
  return p;
}

// Trivial — no awaits, no compute
async function trivial(): number {
  return 42;
}

// Single await
async function inc(): number {
  let x = await presolved(10);
  return x + 1;
}

// Multiple awaits in sequence
async function combine(): number {
  let a = await presolved(100);
  let b = await presolved(200);
  let c = await presolved(33);
  return a + b + c;
}

// await mixed with non-await arithmetic
async function mixed(seed: number): number {
  let x = await presolved(seed);
  let y = x * 2 + 1;
  let z = await presolved(y);
  return z + 100;
}

// Pending promise's `.value` is the seed (default for the type) —
// used to verify the await of a pending promise gets default not
// crash. Real spec would block / suspend; tr's MVP just reads the
// seed.
async function pending_aware(): number {
  let p = new Promise(0);  // pending; seed 0
  let x = await p;
  return x + 5;  // x = 0 (the seed), so result = 5
}

function check(): number {
  if (trivial().value !== 42) { throw "#1 trivial"; }
  if (inc().value !== 11) { throw "#2 inc"; }
  if (combine().value !== 333) { throw "#3 combine"; }
  if (mixed(7).value !== 115) { throw "#4 mixed"; }   // 7→x; y=15; z=15; +100=115
  if (pending_aware().value !== 5) { throw "#5 pending"; }

  // also verify the wrapping: result IS a Promise (state=fulfilled)
  let p = trivial();
  if (p.state !== 1) { throw "#6 wrap state"; }

  return 0;
}
console.log(check());
