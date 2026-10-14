// §15.7 — a FieldDefinition's Initializer is [~Await]: even inside an
// async function it is a fresh function boundary, where `await` is not
// an operator. Under the Module goal the name is reserved too, so the
// field below can only be written without it. A computed key is not
// part of the initializer and keeps the enclosing [Await].
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("async () => class { x = await 1 };")));
console.log(caught(() => eval("async function f() { return class { static y = await 2 }; }")));
console.log(caught(() => eval("async () => class { [await 1] = 1 };")));

async function make() {
  const v = await Promise.resolve(3);
  return [
    v,
    class {
      x = 4;
    },
  ] as const;
}
make().then(([v, C]) => console.log(v, new C().x));
