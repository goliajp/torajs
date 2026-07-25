// RFC 20260725-getiterator-getmethod 刀 6b — the statically-typed
// class-iterator lowering closes its iterator too.
//
// Knife 6 gave the `any` lane its §7.4.9 call, but a `for..of` over a
// source whose class is known at compile time takes a different loop
// shape with its own exit block, so it kept walking away from the
// iterator on a `break`. Same flag, same gate, same kernel — this
// lane just holds its iterator in a typed local rather than an `Any`
// park slot, so it closes through the value-shaped face.
//
// The classes here are deliberately given distinct SHAPES: this
// lowering still resolves an iterator's class by scanning for a
// structural StructId match, so same-shaped classes make it pick an
// arbitrary one. That is a separate recorded defect, pinned by its
// own case; it must not be what makes this one flaky.

let log = "";

class Steps {
  kind = "steps";
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i, done: this.i > 9 };
  }
  return(): { value: number; done: boolean } {
    log = log + "closed;";
    return { value: 0, done: true };
  }
}
class Source {
  marker = 1;
  [Symbol.iterator](): Steps {
    return new Steps();
  }
}

// `break` closes.
let seen = "";
for (const v of new Source()) {
  seen = seen + String(v);
  if (v >= 3) break;
}
console.log("break:", seen, log);

// Running to completion does not — the iterator closed itself.
class Short {
  kind = 0;
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i, done: this.i > 3 };
  }
  return(): { value: number; done: boolean } {
    log = log + "SHOULD-NOT-RUN;";
    return { value: 0, done: true };
  }
}
class ShortSource {
  marker2 = 2;
  [Symbol.iterator](): Short {
    return new Short();
  }
}
log = "";
let seen2 = "";
for (const v of new ShortSource()) seen2 = seen2 + String(v);
console.log("natural:", seen2, log === "" ? "not-closed" : log);

// An iterator declaring no `return` is already closed — §7.4.9 step 4
// ends there rather than throwing.
class NoReturn {
  a = 0;
  b = 0;
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i, done: false };
  }
}
class NoReturnSource {
  marker3 = 3;
  [Symbol.iterator](): NoReturn {
    return new NoReturn();
  }
}
for (const v of new NoReturnSource()) break;
console.log("no-return-method: ok");

// A `break` on the first step closes, and a repeated loop closes each
// iterator it derives.
class Counted {
  a = 0;
  b = 0;
  c = 0;
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i, done: false };
  }
  return(): { value: number; done: boolean } {
    log = log + "c";
    return { value: 0, done: true };
  }
}
class CountedSource {
  marker4 = 4;
  [Symbol.iterator](): Counted {
    return new Counted();
  }
}
log = "";
const cs = new CountedSource();
for (let k = 0; k < 3; k = k + 1) {
  for (const v of cs) break;
}
console.log("repeated:", log);

// A generator still runs to completion untouched.
function* gen() {
  yield 1;
  yield 2;
  yield 3;
}
let g = "";
for (const v of gen()) g = g + String(v);
console.log("generator:", g);
