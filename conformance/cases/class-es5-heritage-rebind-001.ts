// §14.2.3 / §15.7.14 — a class declaration binds twice, and `extends`
// is what proves the two cannot be collapsed even when the class body
// never says its own name. A sibling reaches its parent through the
// container's binding, which is mutable; the class OBJECT the sibling
// was linked to has to keep answering after a later write.

function ctorlessSubclass() {
  let a = 1;
  class P {
    m() {
      return a;
    }
  }
  class Q extends P {
    static u = "q";
  }
  P = null as any;
  console.log(new Q().m(), Q.u, P);
}
ctorlessSubclass();

function constructedBeforeAndAfter() {
  let a = 2;
  class P {
    m() {
      return a;
    }
  }
  class Q extends P {}
  const before: any = new Q();
  P = null as any;
  const after: any = new Q();
  // `after instanceof (Q as any)` belongs here too and is left out:
  // the cast spelling still hits the unclaimed-receiver reject, which
  // is a separate registered boundary.
  console.log(before.m(), after.m(), after instanceof Q);
}
constructedBeforeAndAfter();

// The prototype link the sibling was given is the class object, not
// whatever the container holds later.
function protoLinkSurvives() {
  let a = 3;
  class P {
    who() {
      return ["p", a];
    }
  }
  class Q extends P {
    who2() {
      return "q";
    }
  }
  const saved: any = P;
  P = 7 as any;
  const q: any = new Q();
  console.log(q.who(), q.who2(), Object.getPrototypeOf(Q) === saved, P);
}
protoLinkSurvives();

// A parent that DOES read its own name, same question.
function parentReadsItself() {
  let a = 4;
  class P {
    static self() {
      return [P, a];
    }
  }
  class Q extends P {}
  const saved: any = P;
  P = null as any;
  console.log(saved.self()[0] === saved, saved.self()[1], new Q() instanceof Q, P);
}
parentReadsItself();
