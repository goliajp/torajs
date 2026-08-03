// `new Object(value)` — §20.1.1.1: Object's [[Construct]] with an
// ordinary NewTarget is its [[Call]] (nullish → fresh object, else
// ToObject). r292: rewritten to the call form at desugar; a fn-name
// argument rides the wrapped closure lane (S15.2.2.1_A2 family).
function func() {
  return 1;
}

const n_obj = new Object(func);
console.log(n_obj === func);
console.log((n_obj as any)());

// primitives box, objects pass through identical
console.log(typeof new Object(5), typeof new Object("s"));
const base = { a: 1 };
console.log(new Object(base) === base);

// nullish → fresh plain object (and the zero-arg form still works)
console.log(JSON.stringify(new Object(null)), JSON.stringify(new Object(undefined)));
console.log(JSON.stringify(new Object()));
