// A `const g = function* () { … }` binding lowers through the
// fn-address lane (the initializer is an `Expr::Ident` naming the
// lifted `__genexpr_*`), which put the function address in a bare
// alloca. A closure capturing that name then treats the slot as a
// capture box — a `FnSig` is Copy, so `write_captures` takes that arm
// unconditionally — and a capture box keeps its refcount at
// `slot - 8`. For a bare alloca that word is the callee frame's saved
// `lr`, so the closure's env drop decremented a return address: the
// program died of `EXC_ARM_DA_ALIGN` with `pc == lr == <addr> - 1`,
// silently, exit 138, no output at all.
//
// The lane now yields to the general path when the name is
// escape-captured, the same way it already yields on a forward
// capture box.
function call(f: () => void): void {
  f();
}

function direct(): void {
  const g = function* () {
    yield 1;
  };
  call(() => {
    g().next();
  });
  console.log("direct ok");
}
direct();

// The capture read through two frames, and the generator stepped to
// completion so its own drop runs too.
function twoDeep(): void {
  const g = function* () {
    yield 1;
    yield 2;
  };
  const outer = () => () => {
    const it = g();
    it.next();
    it.next();
    it.next();
  };
  call(outer());
  console.log("two-deep ok");
}
twoDeep();

// Two captured generator bindings in one frame, captured by one
// closure — both slots must be boxes, not neighbours on the stack.
function pair(): void {
  const a = function* () {
    yield "a";
  };
  const b = function* () {
    yield "b";
  };
  let seen = "";
  call(() => {
    seen = String(a().next().value) + String(b().next().value);
  });
  console.log("pair", seen);
}
pair();

// A plain function binding through the same lane still works — this
// is the shape the lane exists for.
function plainFn(): void {
  const h = function () {
    return 7;
  };
  call(() => {
    console.log("plain", h());
  });
}
plainFn();
