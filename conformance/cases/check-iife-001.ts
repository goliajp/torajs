// S158 — immediately-invoked closure expression `(arrow)(...args)`
// per ES 11.2.5. Pre-fix ssa_lower's `resolve_callee` only handled
// `Expr::Ident` and `Expr::Member` callees; `Expr::Closure` callees
// produced by `lift_arrow_fns` panicked with "unsupported callee
// form: Closure". Existing chained-call path (`f(0)(5)`) already
// lowers an Expr::Call callee via the closure/fnsig CallIndirect
// dispatch; widening the gate to also accept `Expr::Closure` callees
// reuses that path verbatim.

// 1) Basic IIFE — single-arg arrow invoked inline.
const r1 = ((x: number) => x * 2)(21);
console.log(r1);

// 2) Multi-arg IIFE.
const r2 = ((a: number, b: number) => a + b)(3, 4);
console.log(r2);

// 3) String-typed IIFE — refcounted return.
const r3 = ((s: string) => s.toUpperCase())("hello");
console.log(r3);

// 4) IIFE capturing an outer binding.
const outer = 100;
const r4 = ((x: number) => x + outer)(5);
console.log(r4);

// 5) Statement-position IIFE — void return.
((msg: string) => console.log("from-iife:" + msg))("hi");

// 6) Nested IIFE — outer arrow's body is another IIFE call. Inner
//    closure ret type isn't propagated to the outer arrow's
//    inferred return (separate L3b wedge), so the outer ann is
//    explicit here.
const r5 = ((x: number): number => ((y: number) => x + y)(10))(20);
console.log(r5);

// 7) IIFE result used in a binop expression.
const r6 = (((x: number) => x * x)(5)) + 1;
console.log(r6);
