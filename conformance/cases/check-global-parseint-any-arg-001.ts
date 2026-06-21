// S337 — bare-name `parseInt(Any, Any?)` per ES §19.2.5 step 1
// (str ToString) + step 2 (radix ToInt32) — both accept arbitrary-
// typed input. Pre-S337 the check.rs strict gate at ~5908 / 5923
// rejected `o: any` operands with 'parseInt arg 0 must be string,
// got Any' / 'parseInt arg 1 must be number, got Any'. Sister to
// S336 (parseFloat bare-name Any) for str slot, S327 (Number.
// parseInt member radix Any) for radix slot. ssa_lower mirror
// routes str-Any through anyv_to_str_pair → any_to_str and
// radix-Any through anyv_to_number → coerce_to_i64.

let counter = 0;
const step = (v: string): string => {
  counter += 1;
  return v;
};

// any str (1-arg)
const a: any = "42";
console.log("a=", parseInt(a));

// any str + literal radix
console.log("b=", parseInt(a, 10));

// any str + any radix
const r: any = 16;
const hex: any = "ff";
console.log("c=", parseInt(hex, r));

// literal str + any radix
console.log("d=", parseInt("ff", r));

// any-from-numeric (ToString)
const n: any = 12345;
console.log("e=", parseInt(n));

// any-from-fn-return — side-effect counter must fire
const giveS = (): any => step("100");
console.log("f=", parseInt(giveS(), 2));
console.log("counter=", counter);

// explicit-undefined str → NaN
console.log("g=", parseInt(undefined));

// explicit-undefined radix folds to auto-detect (default base 10 / 16 for 0x)
console.log("h=", parseInt("0x10", undefined));

// string-literal fast path still works
console.log("i=", parseInt("123", 10));

// NaN-bearing Any radix → 0 → auto-detect
const nan: any = NaN;
console.log("j=", parseInt("42", nan));
