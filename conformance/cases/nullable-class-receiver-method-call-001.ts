// A method call on a `C | undefined` receiver compiles, and an absent
// receiver raises the TypeError before any argument is evaluated.
//
// A class method call is rewritten to `__cm_C__m(receiver, ...)`
// before lowering: the receiver becomes argument 0, typed as the
// class. Two things were wrong with that position:
//  - the checker compared a `Nullable(ClassRef)` argument against the
//    `ClassRef` receiver slot and refused the program;
//  - the lowering's receiver guard only recognised a Member callee, so
//    once the program compiled the method body was inlined straight
//    onto the absent receiver — a SIGSEGV, not a TypeError.
//
// §13.3.6.1 evaluates the member reference first, and GetValue on it
// is where §7.1.20 ToObject throws for an absent base, BEFORE the
// argument list (§13.3.6.2 step 3). The `side()` line below pins that
// order: "SIDE" must not print.
//
// Written out directly, never through a helper (r687).

class C {
  v: number = 3;
  m(): number {
    return this.v + 1;
  }
  n(k: number): number {
    return this.v * k;
  }
}

function maybe(): C | undefined {
  return Math.random() < 2 ? new C() : undefined;
}
function none(): C | undefined {
  return undefined;
}
function side(): number {
  console.log("SIDE");
  return 2;
}

function main(): void {
  const live: C | undefined = maybe();
  console.log(live.m(), live.n(2));

  const gone: C | undefined = none();
  try {
    console.log(gone.m());
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  try {
    const x = gone.n(side());
    console.log(x);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
