// §13.1.1 / §15.8.1 — outside an async body the Script goal reads
// `await` as an IdentifierReference, so a top-level `await x`, an
// `await using` declaration and a `for await` head cannot parse there.
// Eval code is Script code (§19.2.1.1), even inside an async function.
function caught(fn) {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("await 1;")), caught(() => (0, eval)("await 1;")));
console.log(caught(() => eval("await using x = null;")));
console.log(caught(() => eval("for await (const x of []) ;")));
console.log(caught(() => eval("{ await Promise.resolve(1); }")));

// Where the name reading parses, it stands.
console.log(eval("var await = 5; await"));
console.log(eval("await\n;"));

async function inside() {
  const v = await Promise.resolve(4);
  for await (const x of [5]) {
    console.log(v, x);
  }
  console.log(caught(() => eval("await 1;")));
}
inside();
