// §15.7.14 step 3 — the class scope's binding for the class's own
// name is IMMUTABLE, and every method body sits inside that scope. In
// the ES5 lane (a nested class reading an outer local) that write used
// to succeed silently: `typeof C` inside the body answered "number"
// afterwards while the container still held the class. bun throws
// `TypeError: Attempted to assign to readonly property.` — the same
// message a function expression writing its own self-name gets.

function fromStaticMethod() {
  let a = 1;
  class C {
    static bad() {
      C = 1 as any;
      return a;
    }
  }
  try {
    (C as any).bad();
    console.log("no throw");
  } catch (e: any) {
    console.log(e instanceof TypeError, typeof C, (C as any).bad === undefined);
  }
}
fromStaticMethod();

function fromInstanceMethod() {
  let a = 2;
  class C {
    bad() {
      C = 1 as any;
      return a;
    }
  }
  try {
    new C().bad();
    console.log("no throw");
  } catch (e: any) {
    console.log(e instanceof TypeError, typeof C);
  }
}
fromInstanceMethod();

// §13.15.2 takes the rhs reference before PutValue throws, so the
// right-hand side really ran.
function rhsStillRuns() {
  let a = 3;
  let ran = 0;
  class C {
    static bad() {
      C = ((): any => {
        ran = 1;
        return 2;
      })();
      return a;
    }
  }
  try {
    (C as any).bad();
    console.log("no throw");
  } catch (e: any) {
    console.log(e instanceof TypeError, ran, typeof C);
  }
}
rhsStillRuns();

// A body that reads its own name and never writes it is untouched.
function readOnly() {
  let a = 4;
  class C {
    static self() {
      return [C, a];
    }
  }
  console.log(C.self()[0] === C, C.self()[1]);
}
readOnly();

// The container's binding is the other one, and it is writable.
function outerStaysWritable() {
  let a = 5;
  class C {
    static self() {
      return [C, a];
    }
  }
  const D: any = C;
  C = 9 as any;
  console.log(D.self()[0] === D, C);
}
outerStaysWritable();
