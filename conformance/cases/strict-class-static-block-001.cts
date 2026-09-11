// §15.7 — a ClassStaticBlockBody is class code, so it is strict
// whatever the program goal. `desugar_classes_statics` turns each
// block into a top-level `__sb_<C>__<idx>` FnDecl, and the parser used
// to write no directive for it: the per-function strictness probe
// §13.15.2 reads the body's directive prologue, found none, and gave
// every static block the sloppy [[Set]] kernel inside a sloppy Script.
//
// A static METHOD (`bad`) already threw — it is the contrast that says
// the leak was the block's own, not the class's.
//
// Oracle is node, pinned in `.expected`: bun runs a `.cts` entry with
// no strictness at all.
const frozen = Object.freeze({ a: 1 });

function report(label, f) {
  try {
    f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, e.constructor.name);
  }
}

report("static-block   ", function () {
  class A {
    static {
      frozen.a = 2;
    }
  }
});

report("static-method  ", function () {
  class B {
    static bad() {
      frozen.a = 3;
    }
  }
  B.bad();
});

report("block-delete   ", function () {
  class C {
    static {
      delete frozen.a;
    }
  }
});

report("block-nested-fn", function () {
  class D {
    static {
      // §11.2.2 — a function nested in class code is strict too.
      (function () {
        frozen.a = 4;
      })();
    }
  }
});

// Sloppy enclosing code still discards the refusal.
report("sloppy-plain   ", function () {
  frozen.a = 5;
});
console.log("final", frozen.a);
