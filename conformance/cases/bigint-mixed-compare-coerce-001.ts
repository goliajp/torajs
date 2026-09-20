// The two shapes rotation 676's closing sweep caught as WRONG ANSWERS
// while the gate stayed green -- because this repo had no fixture with
// either shape. ch1 §3.3 asks for them by name; here they are.
//
// 1) §7.2.13 IsLessThan has an answer between numeric types, so a
//    BigInt/Boolean pair is an ordinary comparison. Widening the
//    checker alone left a second hand-written type list in the
//    lowering behind, and tr answered `true`.
console.log(1n > true, 1n < true, 1n >= true, 1n <= true);
console.log(0n > false, 0n >= false, 2n > true, 1n == true);

// 2) A typed-throw emitted ahead of time is only equivalent to the
//    original program when nothing observable happens in between --
//    and an object's ToPrimitive is user code that §13.15.3 runs
//    FIRST. So this must be the user's error, not tr's TypeError.
class MyError extends Error {}
const prim = {
  [Symbol.toPrimitive]() {
    throw new MyError("from toPrimitive");
  },
};
try {
  console.log((prim as any) >>> 0n);
} catch (e) {
  console.log((e as Error).constructor.name, (e as Error).message);
}

// The all-primitive pair keeps its compile-time verdict: §13.15.3 has
// nothing to run first, so the TypeError is emitted rather than tested.
try {
  console.log((1n as any) >>> 1);
} catch (e) {
  console.log((e as Error).constructor.name);
}
