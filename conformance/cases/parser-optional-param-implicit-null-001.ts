// V3-18 wedge — `function f(x?: T)` implicit null default per
// TS spec §3.9.2.4. The `?` marks the parameter as omittable
// at call sites; subset already promotes T → __nullable(T) at
// the type-ann level, but pre-fix the call site `f()` failed
// arity check with 'expected 1 argument(s), got 0' because
// the param had no default to inject.
//
// Implementation: in parser.rs the optional-param branch now
// synthesizes `Some(self.ast.add_expr(Expr::Null))` as the
// default when the user writes `?` without an explicit `=`
// — apply_default_args picks it up at every call site that
// omits the trailing arg, just like a user-supplied default.
//
// Subset limitation: only fn-decl and class-method paths get
// the implicit null. Arrow-fn `(x?: T) => ...` falls through
// to the strict-arity error because closure-call lowering of
// Nullable<Number> args has a separate pre-existing bug
// (LLVM verify mismatch on the i64 vs ptr boundary). That
// fix is its own wedge.

// Plain optional after no required params.
function f(x?: number): number { return x ?? 0 }
console.log(f())                       // 0
console.log(f(5))                      // 5

// Optional string.
function greet(name?: string): string {
  return "hi " + (name ?? "world")
}
console.log(greet())                   // hi world
console.log(greet("alice"))            // hi alice

// Optional after a required param.
function tag(prefix: string, suffix?: string): string {
  return prefix + (suffix ?? "")
}
console.log(tag("a"))                  // a
console.log(tag("a", "b"))             // ab

// Multiple trailing optionals — caller may stop at any point.
function many(a?: number, b?: number, c?: number): number {
  return (a ?? 0) + (b ?? 0) + (c ?? 0)
}
console.log(many())                    // 0
console.log(many(1))                   // 1
console.log(many(1, 2))                // 3
console.log(many(1, 2, 3))             // 6

// Class method with optional param.
class C {
  greet(name?: string): string {
    return "hi " + (name ?? "anon")
  }
}
let c = new C()
console.log(c.greet())                 // hi anon
console.log(c.greet("bob"))            // hi bob

// Explicit `?` + explicit `=` keeps the user-supplied default
// (the `?` is then redundant but legal per TS — bun accepts it).
function g(x?: number = 99): number { return x ?? 0 }
console.log(g())                       // 99
console.log(g(7))                      // 7
