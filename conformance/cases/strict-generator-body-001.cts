// §11.2.2 — a generator body inside strict code is strict code, and
// so is one carrying its own directive. The body becomes a state
// machine (`while (true) { if (st === 0) { … } }`) inside a synthetic
// class's `next()`, so a directive written at the body's head is
// lowered into an arm rather than staying at the head — where the
// per-function probe §13.15.2 uses reads it. Every generator
// therefore answered "sloppy" until rotation 632.
//
// Oracle is node, pinned in `.expected`: bun runs a `.cts` entry with
// no strictness at all and a `.js` entry as strict throughout.
//
// Every try/catch below is written inline rather than through a
// `report(label, fn)` helper: passing an arrow that calls a nested
// generator into another function is an unrelated SIGBUS (632-01).
const frozen = Object.freeze({ a: 1 });

// Its own directive, in an otherwise sloppy script.
const own = function* () {
  "use strict";
  frozen.a = 2;
  yield 1;
};
try {
  own().next();
  console.log("own", "no throw");
} catch (e) {
  console.log("own", (e as any).constructor.name);
}

function strictOuter(): void {
  "use strict";
  const inherited = function* () {
    frozen.a = 3;
    yield 1;
  };
  try {
    inherited().next();
    console.log("inherited", "no throw");
  } catch (e) {
    console.log("inherited", (e as any).constructor.name);
  }

  // A yield before the refusal puts the assignment in a later arm.
  const afterYield = function* () {
    yield 1;
    frozen.a = 4;
  };
  try {
    const it = afterYield();
    it.next();
    it.next();
    console.log("after-yield", "no throw");
  } catch (e) {
    console.log("after-yield", (e as any).constructor.name);
  }

  // An exception region routes through the same next().
  const inFinally = function* () {
    try {
      yield 1;
    } finally {
      frozen.a = 5;
    }
  };
  try {
    const it = inFinally();
    it.next();
    it.next();
    console.log("in-finally", "no throw");
  } catch (e) {
    console.log("in-finally", (e as any).constructor.name);
  }
}
strictOuter();

// Sloppy generator: the refusal is discarded, the body runs on.
const sloppy = function* () {
  frozen.a = 6;
  yield frozen.a;
};
console.log("sloppy", sloppy().next().value);
