// P-SURF S2.9 — a `let` / `const` at the top level of a function body
// may not repeat a parameter name (ES §14.2.1). Same accident as the
// duplicate-parameter half: `*foo(a) { let a = 3 }` was refused because
// the two names became two fields of the generator's `__Gen_*` class,
// while `function f(a) { let a = 3 }` was not refused at all.
//
// The refusals live in test262. This pins the legal side, which is
// where a name-comparison check overreaches: a body declaration that
// merely *looks* like it collides.

// an inner block has its own scope — the parameter is only shadowed
// there, which is allowed
function block(a: number): number {
  {
    let a = 9;
    if (a !== 9) return -1;
  }
  return a;
}

// a nested function's body is a different body; its `let a` has nothing
// to do with the outer parameter
function nested(a: number): number {
  function inner(): number {
    let a = 5;
    return a;
  }
  return a + inner();
}

// an arrow body nested in a function body, same reasoning
function arrowNested(a: number): number {
  const f = () => {
    let a = 6;
    return a;
  };
  return a + f();
}

// a body declaration that shares a name with a *different* function's
// parameter
function one(v: number): number {
  return v;
}
function two(w: number): number {
  let v = w * 2;
  return v;
}

// destructuring parameters bind their leaves, and a body `let` naming
// something else is fine — the synthesized `__param_destr_N` holder is
// unspellable, so it can never be the thing that collides
function destr({ p }: { p: number }, [q]: number[]): number {
  let sum = p + q;
  const doubled = sum * 2;
  return doubled;
}

// a loop binding reusing the name in its own scope
function loop(a: number): number {
  let total = 0;
  for (let a = 0; a < 3; a++) {
    total += a;
  }
  return total + a;
}

// a catch binding, which is its own scope too. (Its *sibling* shape —
// reading the parameter again after the try/catch, once a catch binding
// has shadowed it — is a separate open gap, S2.14.)
function caught(a: number): number {
  try {
    throw new Error("x");
  } catch (a) {
    return 100 + a.message.length;
  }
}

// class methods and object-literal methods take the same path
class K {
  m(a: number): number {
    let b = a;
    return b;
  }
  *g(a: number) {
    let b = a * 3;
    yield b;
  }
}
const obj = {
  m(a: number): number {
    let b = a + 1;
    return b;
  },
};

console.log(block(1), nested(2), arrowNested(3));
console.log(one(4), two(5));
console.log(destr({ p: 6 }, [7]));
console.log(loop(8));
console.log(caught(9));
console.log(new K().m(10), [...new K().g(2)], obj.m(11));
