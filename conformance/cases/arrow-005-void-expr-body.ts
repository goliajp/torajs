// V3-18 m1.h.15 — arrow with explicit void return + side-effecting
// expression body. Parser desugars `(...) => expr` into
// `Stmt::Return(Some(expr))`, which is the right shape for value-
// returning arrows. For a void-returning arrow whose body is a void
// expression (e.g. `console.log(x)`), the SSA call still produces
// a dummy i64 0 result; passing that to `Terminator::Ret` from a
// fn declared `: void` made LLVM verify reject the IR.
//
// Fix: in lower_stmt's Stmt::Return arm, when the surrounding fn's
// declared ret type is Void, drop the expression's value and emit
// `ret void` instead of `ret i64 0`.
let log_it = (msg: string): void => console.log(msg)
log_it("hello")
log_it("world")

// Inferred-void arrow (no explicit ret type).
let say = (s: string) => console.log("say: " + s)
say("a")
say("b")

// Void arrow assigned then called via an alias.
let f = log_it
f("via alias")
