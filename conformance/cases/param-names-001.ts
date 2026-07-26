// P-SURF S2.9 — a duplicate parameter name is an early SyntaxError
// (ES §15.1.1; the spec scopes it to strict mode and non-simple lists,
// and TS is always strict). tr used to refuse *one* spelling of this by
// accident — `*m(x = 0, x)` became two same-named fields of the
// generator's `__Gen_*` state-machine class and tripped the
// field-conflict check — while `function f(x = 0, x) {}` was not
// refused at all.
//
// The refusals are negative cases and live in test262. What this fixture
// pins is the side a name-comparison check gets wrong: every list that
// merely *looks* like it repeats a name and must keep working.

// two parameters spelled the same in two different functions
function a(v: number): number {
  return v + 1;
}
function b(v: number): number {
  return v * 2;
}

// a parameter shadowing a module-level name of its own
const v = 100;
function shadow(v: number): number {
  return v;
}

// nested functions each with their own `x`
function outer(x: number): number {
  function inner(x: number): number {
    return x * 10;
  }
  return inner(x) + x;
}

// rest after positionals, and a rest name matching nothing
function rest(a: number, b: number, ...more: number[]): number {
  return a + b + more.length;
}

// several destructuring parameters side by side — each mints its own
// synthesized `__param_destr_N` holder, and those must not collide
function destr({ p }: { p: number }, [q]: number[], { r }: { r: number }): number {
  return p + q + r;
}

// destructuring leaves that repeat a name used in a *different*
// parameter's pattern position is still one name each here
function leaves({ m }: { m: number }, [n]: number[]): number {
  return m + n;
}

// TS parameter properties, which promote to fields but are still
// ordinary distinct parameter names
class Point {
  constructor(
    public x: number,
    public y: number,
    private label: string,
  ) {}
  describe(): string {
    return this.label + ":" + this.x + "," + this.y;
  }
}

// accessors: a setter's single parameter, and a getter with none
class Cell {
  private inner: number = 0;
  get value(): number {
    return this.inner;
  }
  set value(next: number) {
    this.inner = next;
  }
}

// object-literal methods and a generator method, each with its own list
const obj = {
  take(one: number, two: number): number {
    return one - two;
  },
  *gen(one: number, two: number) {
    yield one;
    yield two;
  },
};

// arrows, including one whose parameter shadows the enclosing one
const arrow = (t: number) => (t2: number) => t + t2;

// a parenthesized sequence expression that repeats a name is not a
// parameter list at all — the arrow check has to wait for the `=>`
// before it can tell the two apart
const w = 4;
const seq = (w, w);

console.log(a(1), b(1));
console.log(shadow(7), v);
console.log(outer(3));
console.log(rest(1, 2, 3, 4, 5));
console.log(destr({ p: 1 }, [2], { r: 3 }));
console.log(leaves({ m: 4 }, [5]));
console.log(new Point(1, 2, "P").describe());

const c = new Cell();
c.value = 9;
console.log(c.value);

console.log(obj.take(10, 4), [...obj.gen(1, 2)]);
console.log(arrow(1)(2), seq);
