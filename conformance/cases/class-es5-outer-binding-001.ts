// §14.2.3 / §15.7.14 — a class declaration has two bindings, and the
// ES5 lane (a nested class that reads an outer local) has to keep them
// apart once the body reads its own name: a write through the
// container's binding is invisible to the body's reads.

function staticMethod() {
  let a = 7;
  class C {
    static self() {
      return [C, a];
    }
  }
  const D: any = C;
  C = null as any;
  console.log(D.self()[0] === D, D.self()[1], C);
}
staticMethod();

function staticFieldArrow() {
  let a = 1;
  class C {
    static field: any = () => [C, a];
  }
  const D: any = C;
  C = null as any;
  console.log(D.field()[0] === D, D.field()[1], C);
}
staticFieldArrow();

function staticFieldDirect() {
  let a = 2;
  class C {
    static field: any = [C, a];
  }
  const D: any = C;
  C = null as any;
  console.log(D.field[0] === D, D.field[1], C);
}
staticFieldDirect();

function instanceMethod() {
  let a = 3;
  class C {
    m() {
      return [C, a];
    }
  }
  const D: any = C;
  C = null as any;
  console.log(new D().m()[0] === D, new D().m()[1], C);
}
instanceMethod();

// An instance FIELD whose initialiser reads the class name is still
// refused, loudly and for an unrelated reason — `fnexpr this in
// unclaimed receiver position` — on HEAD as well as here. Registered,
// not covered.

// A body that reads nothing of itself needs one binding, and it is the
// mutable one (rotation 587-04).
function noSelfRead() {
  let a = 6;
  class C {
    static t = a;
  }
  const D: any = C;
  C = 9 as any;
  console.log(D.t, C);
}
noSelfRead();

// A sibling's heritage reads the container's binding, at class-
// definition time — and what it links to survives a later write.
function heritage() {
  let a = 8;
  class P {
    static who() {
      return [P, a];
    }
    m() {
      return "p";
    }
  }
  class Q extends P {
    static u = "q";
  }
  const savedP: any = P;
  P = null as any;
  console.log(new Q() instanceof Q, new Q().m(), Q.u, savedP.who()[0] === savedP, P);
}
heritage();
