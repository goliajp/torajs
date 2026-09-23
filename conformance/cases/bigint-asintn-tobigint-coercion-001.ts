// §21.2.2.{1,2} step 2 is `ToBigInt(value)`, a coercion — so every
// operand shape reaches it and §7.1.13 decides. tr refused anything
// that was not statically `BigInt` and refused the missing slot on the
// arity gate, turning legal programs into compile errors.
console.log(BigInt.asIntN(3, true), BigInt.asIntN(3, false));
console.log(BigInt.asIntN(3, "10"), BigInt.asIntN(3, "    0xa    "));
console.log(BigInt.asUintN(3, "10"), BigInt.asUintN(4, [10n]));
console.log(BigInt.asIntN(4, { valueOf() { return 11n; } }));
console.log(BigInt.asIntN(4, { toString() { return "11"; } }));
const anyVal: any = 7n;
console.log(BigInt.asIntN(4, anyVal), BigInt.asUintN(4, anyVal));

// §7.1.13 rejects: Number, nullish, Symbol — all TypeError at runtime.
for (const bad of [1, undefined, null] as any[]) {
  try { BigInt.asIntN(4, bad); console.log("NO THROW"); }
  catch (e: any) { console.log("throws", e.constructor.name); }
}

// A missing value slot IS undefined (§21.2.2.1 step 1 makes
// ToIndex(undefined) = 0 with nothing observable, then step 2 throws).
try { (BigInt as any).asIntN(); console.log("NO THROW"); }
catch (e: any) { console.log("no-arg", e.constructor.name); }
try { (BigInt as any).asIntN(0); console.log("NO THROW"); }
catch (e: any) { console.log("one-arg", e.constructor.name); }

// Step 1 still runs first and still owns its RangeError (§7.1.22).
try { BigInt.asIntN(-1, 0n); console.log("NO THROW"); }
catch (e: any) { console.log("negative-bits", e.constructor.name); }

// Order of steps: ToIndex(bits) before ToBigInt(value).
const order: string[] = [];
try {
  BigInt.asIntN({ valueOf() { order.push("bits"); return 4; } } as any,
                { valueOf() { order.push("value"); return 1n; } } as any);
} catch (e: any) { order.push("threw"); }
console.log(order.join(","));

// A statically typed BigInt operand keeps the direct path.
const b: bigint = 255n;
console.log(BigInt.asIntN(4, b), BigInt.asUintN(4, b));

