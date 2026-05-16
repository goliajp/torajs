// P3.closure-in-struct-field — function expression assigned to a
// struct field that captures outer locals. Pre-fix tora exits 138
// (SIGBUS): user-source type `() => T` is parsed as FnSig but
// lift_arrow_fns produces a Closure value when captures are non-empty,
// so the struct's field canon (FnSig) and the stored value (Closure)
// disagree at the ABI level — dispatch site uses Load+CallIndirect
// against an env_ptr it thinks is a fn_ptr. L4 trigger #2 for closing
// P3.
//
// The fix is the substrate trio: lift_arrow_fns always emits Closure
// (even with 0 captures), parse_type maps `() => T` to Type::Closure,
// and a new `synthesize_fn_to_closure_forwarders` desugar pass wraps
// any remaining Ident-to-top-FnDecl (FnSig value) in a trivial
// `__forward_<name>(__env, ...) { return <name>(...); }` closure when
// it appears in a Closure-typed slot.

// Single capture, mutated across calls.
type Ctr = { tick: () => number };
function single_capture(): void {
  let cnt: number = 0;
  const obj: Ctr = { tick: function(): number { cnt = cnt + 1; return cnt; } };
  console.log(obj.tick());  // 1
  console.log(obj.tick());  // 2
  console.log(cnt);          // 2
}
single_capture();

// Multiple captures of different primitive types.
type Multi = { read: () => string };
function multi_capture(): void {
  let n: number = 7;
  let s: string = "v";
  const obj: Multi = { read: function(): string { return s + n; } };
  console.log(obj.read());  // v7
  n = 8;
  console.log(obj.read());  // v8
}
multi_capture();

// 1-arg method that also captures.
type Adder = { add: (x: number) => number };
function with_arg(): void {
  let base: number = 100;
  const obj: Adder = { add: function(x: number): number { return base + x; } };
  console.log(obj.add(5));   // 105
  console.log(obj.add(10));  // 110
}
with_arg();

// Method ref escapes to a local + invoked through it (struct-field load
// still required because struct holds the Closure value).
type Box = { run: () => number };
function field_load(): void {
  let v: number = 42;
  const obj: Box = { run: function(): number { return v; } };
  console.log(obj.run());  // 42
}
field_load();
