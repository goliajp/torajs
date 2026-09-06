// §13.5.1.2 step 4.b — a Super Reference operand makes `delete` a
// ReferenceError, whatever the property is and whether or not it
// exists. tr deleted nothing and answered quietly instead: the super
// desugars deliberately leave a delete operand as a plain member off
// a minted base (routing it through the read kernel breaks
// `delete <a call>`), and that plain member is just an ordinary
// delete by the time it reaches lowering. The judgement is now
// recorded by the pass that still knows the operand was `super.x`.

class Base {
  x: number = 1;
  method(): number {
    return 1;
  }
}

class D extends Base {
  viaDot(): void {
    delete super.x;
  }
  viaMethod(): void {
    delete super.method;
  }
}

const d = new D();
try {
  d.viaDot();
  console.log("dot: no throw");
} catch (e: any) {
  console.log("dot", e.constructor === ReferenceError, e.message);
}
try {
  d.viaMethod();
  console.log("method: no throw");
} catch (e: any) {
  console.log("method", e.constructor === ReferenceError);
}

// A property that does not exist on the parent answers the same way —
// the reference is a Super Reference before anything is looked up.
class E extends Base {
  gone(): void {
    delete super.nosuchprop;
  }
}
try {
  new E().gone();
  console.log("absent: no throw");
} catch (e: any) {
  console.log("absent", e.constructor === ReferenceError);
}

// Step 1 evaluates the operand first, so a computed key's side effect
// happens and THEN the throw does.
let n = 0;
class F extends Base {
  computed(): void {
    delete super[((n = 1), "x")];
  }
}
try {
  new F().computed();
  console.log("computed: no throw");
} catch (e: any) {
  console.log("computed", e.constructor === ReferenceError, "n", n);
}

// In a constructor, after super().
class G extends Base {
  constructor() {
    super();
    delete super.x;
  }
}
try {
  new G();
  console.log("ctor: no throw");
} catch (e: any) {
  console.log("ctor", e.constructor === ReferenceError);
}

// Controls: reading and calling through super are untouched, and an
// ordinary property delete still works. (Read a member that is really
// on the prototype: `super.x` names an instance field, and tr answers
// its TYPE's default there where bun answers undefined — a separate
// bug, kept out of this fixture rather than frozen into it.)
class H extends Base {
  read(): string {
    return typeof super.method;
  }
  call(): number {
    return super.method();
  }
}
const h = new H();
console.log("read", h.read(), "call", h.call());
const plain: any = { k: 1 };
console.log("plain", delete plain.k, plain.k);
