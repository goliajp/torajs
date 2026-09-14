// A bare `return;` inside a try whose finally still has to run parks
// no value: the function is Void, and the pending-return slot the
// finally hand-off allocates for other return types has no Void
// form — `Alloca(Void) is not allocable` killed every such program at
// lowering (test262 derived-class-return-override-catch-finally and
// its three siblings, rotation 651). The flag alone carries the
// pending return now.
class C extends class {} {
  constructor() {
    try {
      throw null;
    } catch (e) {
      return;
    } finally {
      super();
    }
  }
}
var o = new C();
console.log(typeof o, o instanceof C);

class D extends class {} {
  constructor() {
    try {
      return;
    } finally {
      super();
      console.log("D finally");
    }
  }
}
console.log(typeof new D());

function f(): void {
  try {
    console.log("body");
    return;
  } finally {
    console.log("finally");
  }
}
f();

function g(n: number): void {
  for (let i = 0; i < n; i++) {
    try {
      if (i === 1) return;
      console.log("i", i);
    } finally {
      console.log("fin", i);
    }
  }
  console.log("unreached");
}
g(3);

function h(): void {
  try {
    try {
      return;
    } finally {
      console.log("inner");
    }
  } finally {
    console.log("outer");
  }
}
h();
console.log("done");
