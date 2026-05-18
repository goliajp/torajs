// P7.2b-1 — `return <any-valued expr>` from a fn whose declared
// return is a concrete primitive. Root cause of the B-throw-2
// garbage: Stmt::Return's coercion had the `ret==Any && actual!=Any
// → box_to_any` direction but not its inverse, so an Any operand
// returned into an I64/F64/Bool slot fell through unchanged and the
// caller received the raw Any-box pointer reinterpreted as the
// primitive (e.g. `catch (e: any) { return e + 1000 }` returned a
// pointer ~4.3e9 instead of 1099).
//
// Fix: symmetric coercion arm calling __torajs_any_to_number
// (JS §7.1.4 ToNumber, one runtime helper, mirrors coerce_to_bool's
// any_to_bool precedent), then F64→i64 narrow for an I64 return.
// Scope is numeric only — for an Any holding a number ToNumber is
// value-preserving and matches bun. Non-numeric declared returns
// (`: boolean` etc.) are a distinct typed-tier-annotation question
// (bun erases the annotation, keeps the raw value) and are out of
// B-throw-2 scope.
//
// Acceptance: the caught-Any value flows back through a numeric
// return correctly — direct, with arithmetic, through a fn-boundary
// throw, into both int- and float-declared returns.

// 1. The exact B-throw-2 shape: catch (e: any) { return e + N }.
function check(n: number): number {
  if (n < 0) {
    throw 99;
  }
  return n;
}
function safe(n: number): number {
  try {
    return check(n);
  } catch (e: any) {
    return e + 1000;
  }
}
console.log(safe(-5)); // 1099
console.log(safe(5));  // 5

// 2. Return the Any binding directly (no arithmetic) into : number.
function passthru(): number {
  try {
    throw 7;
  } catch (e: any) {
    return e;
  }
}
console.log(passthru()); // 7

// 3. Float thrown, caught as any, returned into a : number fn —
//    ToNumber keeps the fractional value, no bitcast garbage.
function f(): number {
  try {
    throw 2.5;
  } catch (e: any) {
    return e + 0.5;
  }
}
console.log(f()); // 3

// 4. Float thrown, caught as any, arithmetic result is integral —
//    proves the Any payload (a real f64, not its bit pattern) feeds
//    ToNumber before the return narrow.
function fmul(): number {
  try {
    throw 2.5;
  } catch (e: any) {
    return e * 4;
  }
}
console.log(fmul()); // 10
