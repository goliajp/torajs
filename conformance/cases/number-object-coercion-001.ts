// `Number(<object>)`, the sibling of `String(<object>)`.
//
// §7.1.4 step 8: ToNumber of an object is ToNumber of whatever
// OrdinaryToPrimitive answers with the NUMBER hint — the receiver's
// own `valueOf` when it has one, and NaN when the walk falls all the
// way through Object.prototype's. The String arm of the same coercion
// table had admitted both object shapes for as long as it had run
// them; the Number arm listed everything except them, so
// `Number({v:5, valueOf(){...}})` was a type error rather than 10.
//
// The kernel that answers this is the one the `any` lane was already
// using. The typed spelling boxes the pointer — a pure encode, no
// refcount traffic — and asks the same question.

function objectWithValueOf(): void {
  const o = {
    v: 5,
    valueOf(): number {
      return this.v * 2;
    },
  };
  console.log(Number(o));
}

class Money {
  cents: number = 250;
  valueOf(): number {
    return this.cents;
  }
}

function classWithValueOf(): void {
  console.log(Number(new Money()));
}

// no hook: the walk reaches Object.prototype and the answer is NaN,
// not an error
class Bare {
  v: number = 7;
}

function noHook(): void {
  const plain = { v: 5 };
  console.log(Number(plain), Number(new Bare()));
}

// a hook that answers a non-number is ToNumber'd in turn
function stringyValueOf(): void {
  const o = {
    valueOf(): string {
      return "42";
    },
  };
  console.log(Number(o));
}

// a throwing hook is catchable
class Boom {
  valueOf(): number {
    throw new Error("boom");
  }
}

function throwingHook(): void {
  try {
    console.log(Number(new Boom()));
  } catch (e: any) {
    console.log("caught", e.message);
  }
}

// unchanged ground: everything the arm already took
function otherShapes(): void {
  console.log(Number(1), Number(true), Number(null), Number("3.5"));
  console.log(Number([]), Number([7]), Number([1, 2]));
  const a: any = "8";
  console.log(Number(a));
}

// repeated: the receiver is borrowed, so it is still there afterwards
// (the result stays in a fresh `const` each turn — a `let` seeded with
// an integer literal cannot take an f64 answer yet, which is true of
// `Number("3.5")` too and is a width-analysis gap of its own)
function repeats(): number {
  const m = new Money();
  let seen = 0;
  for (let i = 0; i < 500; i++) {
    const n = Number(m);
    if (n > 0) {
      seen = seen + 1;
    }
  }
  return seen + m.cents;
}

function main(): void {
  objectWithValueOf();
  classWithValueOf();
  noHook();
  stringyValueOf();
  throwingHook();
  otherShapes();
  console.log(repeats());
}

main();
