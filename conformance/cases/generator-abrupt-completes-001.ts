// A generator whose body throws is COMPLETED — ES §27.5.1.2. Every
// later next() answers `{ value: undefined, done: true }`; it does NOT
// re-enter the body.
//
// The state machine persisted its resume label on every internal
// transition, so a throw left the field pointing at the arm the throw
// escaped from and the next call re-ran it — raising the same error
// again, forever. Now only a YIELD writes the field back, so any other
// way out of next() (a throw, an early return, running off the end)
// leaves the generator dead.

let entered = 0;
let past = 0;

function* boom(): Generator<number> {
  entered = entered + 1;
  throw new Error("boom");
  past = past + 1;
}

const it = boom();
try {
  it.next();
  console.log("no throw");
} catch (e) {
  console.log("caught:", e.message);
}

// Closed: the body does not run a second time.
const after = it.next();
console.log("after.done", after.done, "entered", entered, "past", past);

const again = it.next();
console.log("again.done", again.done, "entered", entered);

// Destructuring forwards the abrupt completion and leaves the iterator
// closed, which is what test262's ary-ptrn-*-iter-step-err assert.
let steps = 0;
function* counted(): Generator<number> {
  steps = steps + 1;
  throw new Error("boom");
}
const c = counted();
try {
  const [...rest] = c;
  console.log("no throw", rest.length);
} catch (e) {
  console.log("dstr caught:", e.message);
}
console.log("c.next().done", c.next().done, "steps", steps);

// A generator that completes NORMALLY is equally dead, and one that is
// merely suspended at a yield still resumes — the label is persisted on
// that path alone.
function* three(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}
const t = three();
console.log(t.next().value, t.next().value, t.next().value);
console.log("t.done", t.next().done, t.next().done);

// Resuming across a loop back-edge still works: the internal goto moves
// a local cursor, and only the yield persists.
function* upto(n: number): Generator<number> {
  let i = 0;
  while (i < n) {
    yield i;
    i = i + 1;
  }
}
const acc: number[] = [];
for (const v of upto(4)) {
  acc.push(v);
}
console.log(acc.join(","));

// The generator's own return() closes it, and stays closed.
const r = three();
console.log(r.next().value, r.return(0).done, r.next().done);
