// An object literal's method reached through a COERCION rather than
// through the call site the source wrote.
//
// `o.toString()` works: the method's receiver is its first declared
// param and the static call site passes it. `String(o)` does not go
// that way — §7.1.17 hands the object to OrdinaryToPrimitive, which
// finds `toString` at runtime and calls it through the uniform boxed
// adapter. That adapter takes the receiver in argv[0] only when the
// closure cell says so, and the typed lane never said so: `this` came
// in undefined and reading a field off it was a segfault, with no
// output and no message.
//
// The any-lane sibling of this literal already declared it. Both lanes
// put the receiver in the same place, so both say the same thing now.

// the three coercion positions that reach toString
function coercions(): void {
  const o = {
    v: 5,
    toString(): string {
      return "O(" + this.v + ")";
    },
  };
  console.log(String(o));
  console.log(`${o}`);
  console.log("x" + o);
  // unchanged ground: the call the source wrote
  console.log(o.toString());
}

// a method that reads several fields, and one that calls a sibling —
// both go through the same receiver
function readsSiblings(): void {
  const o = {
    a: 2,
    b: 3,
    sum(): number {
      return this.a + this.b;
    },
    toString(): string {
      return "sum=" + this.sum();
    },
  };
  console.log(String(o), o.sum());
}

// the receiver and a capture at once: the env still carries `c`, the
// receiver still arrives beside it
function capturesToo(): string {
  const tag = "T";
  const o = {
    v: 9,
    toString(): string {
      return tag + this.v;
    },
  };
  return String(o);
}

// a this-free method must NOT change shape — plenty of consumers hand
// such a closure on with no receiver at all, and giving it one shifts
// every argument
function thisFreeMethod(): void {
  const o = {
    v: 4,
    plain(): number {
      return 11;
    },
    toString(): string {
      return "v" + this.v;
    },
  };
  console.log(o.plain(), String(o));
}

// no hook at all is still the §20.1.4.4 default
function noHook(): void {
  const o = { v: 1 };
  console.log(String(o));
}

// hint string takes toString first even when valueOf is also there
function bothHooks(): void {
  const o = {
    v: 6,
    toString(): string {
      return "S" + this.v;
    },
    valueOf(): number {
      return this.v;
    },
  };
  console.log(String(o), `${o}`);
}

// the any lane reaches the same method through the dynobj dispatcher
function anyLane(): void {
  const o: any = {
    v: 5,
    toString(): string {
      return "A" + this.v;
    },
  };
  console.log(String(o), `${o}`);
}

// many coercions in a row: the receiver is borrowed, not consumed
function repeats(): string {
  const o = {
    v: 2,
    toString(): string {
      return "R" + this.v;
    },
  };
  let acc = "";
  for (let i = 0; i < 500; i++) {
    acc = String(o);
  }
  return acc;
}

function main(): void {
  coercions();
  readsSiblings();
  console.log(capturesToo());
  thisFreeMethod();
  noHook();
  bothHooks();
  anyLane();
  console.log(repeats());
}

main();
