// Two `let`s of the same name in different scopes of one generator.
//
// Every `let` in a generator body lifts to a field of the iterator
// class, and fields live in one flat namespace, so both `ee`s below
// mapped onto `this.ee`. The desugar refused to guess and aborted:
// "duplicate `let ee` declarations across scopes — rename one". A
// program reaches that without trying — the test262 async harness
// port writes exactly this catch shape twice in one function — and
// two counting loops in a row reach it just as easily.
//
// The second declaration is renamed now. A `let` is visible from its
// own declaration to the end of the enclosing statement list, so
// renaming across exactly that range renames exactly its own
// references, which is also why the shadowing cases below stay
// correct: an inner block that redeclares the name gets its own
// rename, and reads after the block still see the outer binding.
//
// One shape still aborts, and loudly: a range that declares the name
// AGAIN deeper inside, where a rename would have to stop at the inner
// declaration. Not exercised here — an abort has no output to compare.

function* g(): number {
  // sibling catch scopes
  try {
    throw new Error("a");
  } catch (e) {
    const ee: any = e;
    yield ee.message.length;
  }
  try {
    throw new Error("bb");
  } catch (e) {
    const ee: any = e;
    yield ee.message.length;
  }

  // sibling loop counters — the binding's range is the whole `for`,
  // not a slice of the enclosing list
  for (let i = 0; i < 2; i = i + 1) {
    yield 100 + i;
  }
  for (let i = 0; i < 2; i = i + 1) {
    yield 200 + i;
  }

  // shadowing: the inner declaration is the one that moves, and the
  // outer binding is still readable on both sides of it
  const x: number = 1;
  yield x;
  {
    const x: number = 10;
    yield x;
  }
  yield x;
  if (x === 1) {
    const x: number = 100;
    yield x;
  }
  yield x;

  // a name declared only once is untouched
  const only: string = "once";
  yield only.length;
}

for (const v of g()) {
  console.log(v);
}
