// `String(<class instance>)` and its two siblings, `${c}` and
// `"x" + c`.
//
// The runtime always knew how to do this — an `any`-typed reference to
// the same instance answered correctly. Two things stood in the way of
// the typed spelling. The checker's coercion table listed every object
// shape but ClassRef, so `String(c)` never reached lowering. And the
// lowering decided whether to run OrdinaryToPrimitive by looking for a
// `toString` / `valueOf` FIELD in the struct layout — a class keeps its
// methods on the prototype, never in the layout, so that test answered
// no for every class and would have printed "[object Object]" over a
// user `toString`.
//
// The layout test is gone rather than extended: the runtime is what
// knows whether a hook exists, and it answers the §20.1.4.4
// "[object Object]" itself through Object.prototype.toString when there
// is none.
//
// Writing this case turned up a third one, on the plain call the source
// writes rather than on any coercion — see `siblingNames` below.

class Point {
  x: number = 1;
  y: number = 2;
  toString(): string {
    return "(" + this.x + "," + this.y + ")";
  }
}

// the three coercion positions
function coercions(): void {
  const p = new Point();
  console.log(String(p));
  console.log(`${p}`);
  console.log("at " + p);
  // unchanged ground: the call the source wrote
  console.log(p.toString());
}

// a class with no hook of its own is the spec default, not an error
class Bare {
  v: number = 7;
}

function noHook(): void {
  const b = new Bare();
  console.log(String(b), `${b}`, "x" + b);
}

// an inherited hook counts: the lookup walks the prototype chain
class Base {
  toString(): string {
    return "Base";
  }
}
class Derived extends Base {
  n: number = 3;
}

function inherited(): void {
  const d = new Derived();
  console.log(String(d), "y" + d, d.toString());
}

// an override wins over the inherited one
class Override extends Base {
  toString(): string {
    return "Override";
  }
}

function overridden(): void {
  console.log(String(new Override()), new Override().toString());
}

// several unrelated classes declaring the SAME name is what made the
// direct call go wrong too: desugar leaves those calls Member-shaped
// for the sibling-class lane to resolve by the receiver's static
// class, and the Object.prototype arm sitting in front of that lane
// answered "[object Object]" for every one of them. The other four
// names it covers were shadowed the same way.
class Money {
  cents: number = 250;
  valueOf(): number {
    return this.cents;
  }
  hasOwnProperty(k: string): boolean {
    return k === "cents";
  }
}
class Weight {
  grams: number = 40;
  valueOf(): number {
    return this.grams;
  }
  hasOwnProperty(k: string): boolean {
    return k === "grams";
  }
}

function siblingNames(): void {
  const m = new Money();
  const w = new Weight();
  console.log(m.valueOf(), w.valueOf());
  console.log(m.hasOwnProperty("cents"), w.hasOwnProperty("cents"));
  console.log(new Point().toString(), new Base().toString());
}

// a throwing hook is a catchable exception, not a crash
class Boom {
  toString(): string {
    throw new Error("boom");
  }
}

function throwingHook(): void {
  try {
    console.log(String(new Boom()));
  } catch (e: any) {
    console.log("caught", e.message);
  }
}

// unchanged ground: the object shapes that already worked, all three
// still answering what they answered
function otherShapes(): void {
  const plain = { v: 5 };
  console.log(String(plain), "x" + plain, `${plain}`);
  const hooked = {
    v: 5,
    toString(): string {
      return "P" + this.v;
    },
  };
  console.log(String(hooked), "x" + hooked, `${hooked}`);
  const xs = [1, 2, 3];
  console.log(String(xs), "x" + xs);
  console.log(String(1), String(true), String("s"), String(null));
}

// repeated coercion of one instance: the receiver is borrowed, so the
// instance is still there afterwards
function repeats(): string {
  const p = new Point();
  let acc = "";
  for (let i = 0; i < 500; i++) {
    acc = String(p);
  }
  return acc + p.x;
}

function main(): void {
  coercions();
  noHook();
  inherited();
  overridden();
  throwingHook();
  siblingNames();
  otherShapes();
  console.log(repeats());
}

main();
