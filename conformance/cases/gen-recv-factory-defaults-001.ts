// A generator object's receiver is knowable, so `it.next()` must not
// depend on nobody else in the program owning the name `next`.
//
// Method-default padding for `obj.m()` asks a table keyed by the bare
// method NAME, because a receiver of unknown static type has no other
// way to find its callee. Two owners whose defaults disagree evict the
// name — an honest arity error rather than a wrong pad, which is the
// right trade, except that the receiver usually IS knowable and then
// nothing should have been asking the shared table at all.
//
// `new C()` receivers already resolved through their class. Generator
// objects did not: `desugar_generators` turns `function* g()` into a
// `__Gen_g` class plus the thin factory
// `function g(args) { return new __Gen_g(args); }`, and a call of that
// factory named no class. So a single class declaring `next(step = 5)`
// took EVERY generator in the program down with it — `it.next()`
// failed to compile with "expected 1 argument(s), got 0" while the
// neighbouring `c.next()` worked.
//
// The generator trio `next` / `return` / `throw` are all ordinary
// method names a user class may also declare, so all three are below.

class Cursor {
  next(step: number = 5): number {
    return step;
  }
  return(v: number = 7): number {
    return v;
  }
  throw(v: number = 9): number {
    return v;
  }
}

const c = new Cursor();
console.log(c.next(), c.return(), c.throw());

function* g(): number {
  yield 1;
  yield 2;
}

const it = g();
console.log(it.next().value, it.next().value);

// the factory result used directly as a receiver, no binding
console.log(g().next().value);

// for-of over the same generator, which drives next() internally
for (const v of g()) {
  console.log("forof", v);
}

// a generator that reads what next() sends, still padded correctly
function* echo(): number {
  const a = yield 1;
  console.log("sent", a);
}
const ie = echo();
ie.next();
ie.next(42);

// a hand-written thin factory resolves the same way
class Box {
  scale(by: number = 3): number {
    return by;
  }
}
function makeBox(): Box {
  return new Box();
}
const b = makeBox();
console.log(b.scale());

// and a second owner of `scale` with a different default, to prove
// the shared table is genuinely evicted and the receiver is what
// answers
class Ruler {
  scale(by: number = 11): number {
    return by;
  }
}
const r = new Ruler();
console.log(r.scale());
