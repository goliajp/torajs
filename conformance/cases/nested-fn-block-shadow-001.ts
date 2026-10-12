// A block that declares the name of a lifted nested function keeps its
// own binding (§14.2.2: a Block's lexical declarations shadow the
// enclosing ones for the whole block). The lift renames every reference
// to the declaration, and the walk that did it went straight through
// such a block, so the block's `f()` called the lifted declaration.

// A nested generator declaration, lifted early by the generator lane.
function viaGenerator(): number {
  function* f() { yield 1; }
  {
    const f = () => 9;
    console.log(f());
  }
  return f().next().value as number;
}
console.log(viaGenerator());

// Deeper: the shadowing block is not the one holding the reference.
function deeper(): number {
  function* f() { yield 2; }
  {
    let f = () => 8;
    {
      console.log(f());
    }
  }
  return f().next().value as number;
}
console.log(deeper());

// A catch parameter and a switch's clauses are scopes too.
function catchParam(): number {
  function* f() { yield 3; }
  try {
    throw () => 7;
  } catch (f) {
    const thrown: any = f;
    console.log(thrown());
  }
  return f().next().value as number;
}
console.log(catchParam());

function caseBlock(k: number): number {
  function* f() { yield 4; }
  switch (k) {
    case 0:
      const f = () => 6;
      console.log(f());
  }
  return f().next().value as number;
}
console.log(caseBlock(0));

// The plain declaration takes the other lift; same answer.
function plain(): number {
  function f() { return 1; }
  {
    const f = () => 9;
    console.log(f());
  }
  return f();
}
console.log(plain());
