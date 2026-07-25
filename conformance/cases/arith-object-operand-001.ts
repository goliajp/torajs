// An object on one side of `-` `*` `/` `%`, and under unary `+` / `-`.
//
// §13.7-§13.10 call ToNumeric on each operand unconditionally, and
// §13.5.4 / §13.5.5 are ToNumber on theirs — so the object runs its
// `valueOf` and answers NaN when it has none. Both were rejected:
// "arithmetic requires number or bigint operands". The `any` lane had
// answered all of them correctly the whole time; the kernel that does
// it is the same one, reached by boxing the pointer (a pure encode,
// no refcount traffic).
//
// `+` is deliberately NOT part of this. Its ToPrimitive uses the
// DEFAULT hint, whose answer for a hook-free object is a STRING —
// `{v:1} + 1` is "[object Object]1", not NaN — so the result type is
// not something this static lane can promise. Ordering comparison has
// the same wrinkle for a string operand and is left alone too.

// (each function writes its own literal: a top-level object literal
// carrying a method is not visible from a function body yet, which is
// a scoping gap of its own and nothing to do with arithmetic)

class Money {
  cents: number = 250;
  valueOf(): number {
    return this.cents;
  }
}

function binaryOps(): void {
  const scaled = {
    v: 5,
    valueOf(): number {
      return this.v * 2;
    },
  };
  console.log(scaled * 3, scaled - 1, scaled / 2, scaled % 3);
}

function classInstance(): void {
  const m = new Money();
  console.log(m * 2, m - 50, m / 5, m % 7);
}

function objectOnEitherSide(): void {
  const scaled = {
    v: 5,
    valueOf(): number {
      return this.v * 2;
    },
  };
  console.log(3 * scaled, 100 - scaled, 100 / scaled);
}

function bothSidesObjects(): void {
  const scaled = {
    v: 5,
    valueOf(): number {
      return this.v * 2;
    },
  };
  const m = new Money();
  console.log(m / scaled);
}

function unary(): void {
  const scaled = {
    v: 5,
    valueOf(): number {
      return this.v * 2;
    },
  };
  console.log(+scaled, -scaled);
  console.log(+new Money(), -new Money());
}

// no hook: ToNumber falls through to NaN rather than erroring
function noHook(): void {
  const bare = { v: 1 };
  console.log(bare * 2, +bare, -bare);
}

// a hook answering a non-number is ToNumber'd in turn
function stringyValueOf(): void {
  const o = {
    valueOf(): string {
      return "21";
    },
  };
  console.log(o * 2, +o);
}

// a throwing hook is catchable
class Boom {
  valueOf(): number {
    throw new Error("boom");
  }
}

function throwingHook(): void {
  try {
    console.log(new Boom() * 2);
  } catch (e: any) {
    console.log("caught", e.message);
  }
}

// unchanged ground: everything the arm already took
function otherShapes(): void {
  console.log(7 * 3, 7 - 3, 7 / 2, 7 % 3);
  console.log(true * 2, null * 2, +true, -null);
  const a: any = 6;
  console.log(a * 2, +a);
  console.log(2n * 3n, -2n);
}

// repeated: the operand is borrowed, so it is still there afterwards
function repeats(): number {
  const m = new Money();
  let seen = 0;
  for (let i = 0; i < 500; i++) {
    if (m * 2 > 0) {
      seen = seen + 1;
    }
  }
  return seen + m.cents;
}

function main(): void {
  binaryOps();
  classInstance();
  objectOnEitherSide();
  bothSidesObjects();
  unary();
  noHook();
  stringyValueOf();
  throwingHook();
  otherShapes();
  console.log(repeats());
}

main();
