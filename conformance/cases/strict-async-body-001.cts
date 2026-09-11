// §11.2.2 — an async function body inside strict code is strict code,
// and so is one carrying its own directive. `desugar_async` wraps the
// whole body in a `Stmt::Try` so a throw becomes a rejected promise,
// which put the directive prologue one level down: the per-function
// probe §13.15.2 uses read the head of the body it was lowering,
// found the Try, and answered "sloppy" for every async function.
//
// Oracle is node, pinned in `.expected`: bun runs a `.cts` entry with
// no strictness at all and a `.js` entry as strict throughout.
const frozen = Object.freeze({ a: 1 });

async function report(label: string, f: any): Promise<void> {
  try {
    await f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, (e as any).constructor.name);
  }
}

const ownArrow = async () => {
  "use strict";
  frozen.a = 2;
};

const ownFn = async function () {
  "use strict";
  frozen.a = 3;
};

function strictOuter(): any[] {
  "use strict";
  const arrow = async () => {
    frozen.a = 4;
  };
  const fn = async function () {
    frozen.a = 5;
  };
  return [arrow, fn];
}

// Sloppy enclosing code keeps discarding the refusal.
const sloppyArrow = async () => {
  frozen.a = 6;
};

async function main(): Promise<void> {
  const [inhArrow, inhFn] = strictOuter();
  await report("own-arrow      ", ownArrow);
  await report("own-fn         ", ownFn);
  await report("inherited-arrow", inhArrow);
  await report("inherited-fn   ", inhFn);
  await report("sloppy-arrow   ", sloppyArrow);
}
main();
