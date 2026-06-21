// S307 — Number/String/Boolean(value, ...trailing) callable-ctor trailing-
// arg ignore per ES §21.1.1 / §22.1.1 / §20.3.1. check.rs S251 already
// typecheck-and-drops, but ssa_lower's three return paths (empty / bare-
// ident undef / lowered args[0]) all read only args[0] without lowering
// trailing — step()-counter probe falsified prior coverage (bun calls 1x,
// tora calls 0x silent-drop).

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

// Boolean(value, trailing)
calls = 0;
const b1 = Boolean(true, step("b1") as any);
console.log("Boolean calls=", calls, "v=", b1);

calls = 0;
const b2 = Boolean(0, step("b2a") as any, step("b2b") as any);
console.log("Boolean-2 calls=", calls, "v=", b2);

// Number(value, trailing)
calls = 0;
const n1 = Number(5, step("n1") as any);
console.log("Number calls=", calls, "v=", n1);

calls = 0;
const n2 = Number("3.14", step("n2a") as any, step("n2b") as any);
console.log("Number-2 calls=", calls, "v=", n2);

// String(value, trailing)
calls = 0;
const s1 = String("hi", step("s1") as any);
console.log("String calls=", calls, "v=", s1);

calls = 0;
const s2 = String(42, step("s2a") as any, step("s2b") as any);
console.log("String-2 calls=", calls, "v=", s2);

// undef bare-ident path
calls = 0;
const n3 = Number(undefined, step("u1") as any);
console.log("Number-undef calls=", calls, "v=", n3);

calls = 0;
const s3 = String(undefined, step("u2") as any, step("u3") as any);
console.log("String-undef calls=", calls, "v=", s3);
