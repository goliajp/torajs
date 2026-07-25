// Printing a `void` / `undefined` value from inside a function body.
//
// A `void` function's [[Call]] result is `undefined` (ES §10.2.1.4
// step 11), and `undefined` prints as the word. tr printed `0` for
// every one of these — but only inside a function body: the
// statement-level fast path already had a checker-type gate for the
// shape, so the very same two lines at top level answered correctly.
// That is what made it look like an `await` bug when it first showed
// up (`const v = await f()` on a `Promise<void>`); the sync form
// hidden in a function body diverges identically.
//
// `Promise<void>.value` — what `await` desugars to — is typed `Void`
// rather than `Undefined`, so both spellings of "nothing" have to be
// recognised at the print site.

let n = 0;

function bump(): void {
  n = n + 1;
}

function sync() {
  // the call itself still has to run for effect
  console.log(bump());
  console.log(n);

  const v = bump();
  console.log(v);
  console.log("v is", v);
  console.log(typeof v);
  console.error(v);
  console.warn(v);
  console.log(n);
}

sync();

// same two lines at top level — these were already correct, and stay
// correct
const t = bump();
console.log(t);
console.log(n);

async function af(flag: boolean): Promise<void> {
  if (flag) {
    return;
  }
}

async function main() {
  const a = await af(true);
  console.log(a);
  console.log(typeof a);
  console.log("a is", a);

  const b = await af(false);
  console.log(b);
}

main();
