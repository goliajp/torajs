// Three statement-level early errors.
// §14.13.1 — a label may not be reused by a statement nested in the
// one it labels (a function body starts a new label set).
// §15.7 — a class static block takes no `return`
// (ClassStaticBlockStatementList is StatementList[~Return]).
// §15.7.1 — nor `arguments`, which a nested class's computed key
// reads from the static block's scope.
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("x: x: 0;")), caught(() => eval("x: { x: 0; }")));
console.log(caught(() => eval("x: for (;;) { y: { x: break x; } }")));
console.log(caught(() => eval("class C { static { x: x: 0; } }")));
console.log(caught(() => eval("class C { static { return; } }")));
console.log(caught(() => eval("class C { static { if (1) { return; } } }")));
console.log(caught(() => eval("class C { static { (class { [arguments]() {} }); } }")));
console.log(caught(() => eval("class C { static { (class extends (arguments, Object) {}); } }")));

// Sibling labels, a label reused inside a nested function, and a
// `return` inside a function the block declares all stay legal.
const log: string[] = [];
x: {
  log.push("a");
}
x: {
  log.push("b");
  break x;
}
y: {
  function inner() {
    y: {
      return "inner";
    }
  }
  log.push(inner());
}
class D {
  static v = 0;
  static {
    const f = () => {
      return 1;
    };
    function g() {
      return 2;
    }
    D.v = f() + g();
    (class {
      [String("k")]() {}
    });
  }
}
console.log(log.join(","), D.v);
