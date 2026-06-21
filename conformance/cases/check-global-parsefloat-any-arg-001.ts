// S336 — bare-name `parseFloat(Any)` per ES §19.2.4 step 1:
// ToString accepts arbitrary-typed input. Pre-S336 the check.rs
// strict gate at ~5944 rejected `o: any` operands with
// 'parseFloat arg must be string, got Any'. Sister to S330
// (Number.parseFloat member method same widen). ssa_lower
// mirror routes Any through anyv_to_str_pair → any_to_str →
// num_parse_float.

let counter = 0;
const step = (v: string): string => {
  counter += 1;
  return v;
};

// any-from-binding (string)
const a: any = "3.14";
console.log("a=", parseFloat(a));

// any-from-binding (numeric)
const b: any = 42;
console.log("b=", parseFloat(b));

// any-from-fn-return — side-effect counter must fire
const giveS = (): any => step("2.5e2");
console.log("c=", parseFloat(giveS()));
console.log("counter=", counter);

// string-literal fast path still works
console.log("d=", parseFloat("123.45"));

// explicit-undefined → NaN (S226 carve-out)
console.log("e=", parseFloat(undefined));

// trailing arg silent-drop with Any first
const arg2 = (): number => {
  counter += 1;
  return 999;
};
console.log("f=", parseFloat(a, arg2() as any));
console.log("counter2=", counter);
