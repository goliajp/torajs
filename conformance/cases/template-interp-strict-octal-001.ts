// Annex B §B.1.2 — a legacy octal escape is a SyntaxError in strict
// code, and a string literal inside a template's `${…}` is no
// exception. Module code is strict, and so is the eval code it runs.
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("`${'\\07'}`;")), caught(() => eval("`a${1}b${'\\101'}`;")));
console.log(caught(() => eval("`${`${'\\07'}`}`;")));

// Hex and unicode escapes inside the interpolation stay legal.
console.log(`${"\x41"}${"\u0042"}`, `${`${"\x43"}`}`);
