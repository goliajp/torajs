// Phase L.1 — Promise<T> class as plain tr code (no compiler magic).
// MVP scope: eager-fire model without an event loop or callback queue.
//
// Limitations of this first cut:
//   - constructor takes a SEED value (not a JS-spec-style executor
//     function) since closure-capture-of-this isn't reliable enough yet.
//   - .then / .catch / .finally fire the callback synchronously when
//     the promise is in the matching settled state at the time of the
//     call. Promises that haven't settled when .then() runs are
//     effectively no-ops (real spec stores callbacks in a queue and
//     fires them on subsequent resolve/reject).
//   - cb has signature `(v: T) => void` — no chaining yet (chain would
//     need to default-construct a Promise<U> with no U seed; deferred
//     until tr supports `new Class<U>()` typed-default).
//   - Yield types stay in `number` space.
//   - `.catch` and `.finally` are now valid method names (parser was
//     extended to allow contextual-keyword tokens after `.`).

class Promise<T> {
  state: number;        // 0 pending, 1 fulfilled, 2 rejected
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
  do_reject(r: T): void {
    if (this.state === 0) {
      this.state = 2;
      this.value = r;
    }
  }
  is_fulfilled(): boolean { return this.state === 1; }
  is_rejected(): boolean { return this.state === 2; }
  is_pending(): boolean { return this.state === 0; }
  then(cb: (v: T) => void): void {
    if (this.state === 1) {
      cb(this.value);
    }
  }
  catch(cb: (r: T) => void): void {
    if (this.state === 2) {
      cb(this.value);
    }
  }
  finally(cb: () => void): void {
    if (this.state !== 0) {
      cb();
    }
  }
}

function promise_resolved(v: number): Promise<number> {
  let p = new Promise(v);
  p.do_resolve(v);
  return p;
}

function promise_rejected(v: number): Promise<number> {
  let p = new Promise(v);
  p.do_reject(v);
  return p;
}

// Top-level callbacks (closures over mutable lets aren't yet reliable
// in tr — `闭包 i64 mutation 不持久` per the project memory).
function print_v(v: number): void {
  console.log(v);
}

function print_done(): void {
  console.log(-1);
}

function check(): number {
  // Resolved promise — then fires, catch and finally don't fire catch
  let p1 = promise_resolved(7);
  if (!p1.is_fulfilled()) { throw "#1 fulfilled state"; }
  if (p1.is_rejected())   { throw "#2 not rejected"; }
  if (p1.is_pending())    { throw "#3 not pending"; }

  // Rejected promise — catch fires
  let p2 = promise_rejected(99);
  if (!p2.is_rejected()) { throw "#4 rejected state"; }
  if (p2.is_fulfilled()) { throw "#5 not fulfilled"; }

  // Pending promise — neither then nor catch fire; finally doesn't either
  let p3 = new Promise(0);
  if (!p3.is_pending()) { throw "#6 pending state"; }

  // After do_resolve, then-cb fires synchronously
  let p4 = new Promise(0);
  p4.do_resolve(11);
  if (!p4.is_fulfilled()) { throw "#7 mutate fulfilled"; }
  // observable side effect via stdout
  p4.then(print_v);   // prints 11

  // After do_reject, catch-cb fires
  let p5 = new Promise(0);
  p5.do_reject(22);
  if (!p5.is_rejected()) { throw "#8 mutate rejected"; }
  p5.catch(print_v);  // prints 22

  // finally fires for both fulfilled and rejected, not pending
  let p6 = promise_resolved(33);
  p6.finally(print_done);  // prints -1

  let p7 = promise_rejected(44);
  p7.finally(print_done);  // prints -1

  let p8 = new Promise(0);
  p8.finally(print_done);  // does NOT print (pending)

  // do_resolve is idempotent — second call is no-op
  let p9 = new Promise(0);
  p9.do_resolve(100);
  p9.do_resolve(200);
  if (p9.value !== 100) { throw "#9 idempotent resolve"; }

  // do_reject after do_resolve also no-op
  let pa = new Promise(0);
  pa.do_resolve(1);
  pa.do_reject(2);
  if (pa.value !== 1) { throw "#10 reject after resolve"; }
  if (!pa.is_fulfilled()) { throw "#11 still fulfilled"; }

  return 0;
}
console.log(check());
