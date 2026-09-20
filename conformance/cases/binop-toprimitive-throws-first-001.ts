// Band B replaces an operator with the throw the spec mandates for its
// operand pair. That is only the same program when nothing observable
// happens in between — and §13.15.3 runs ToPrimitive on BOTH operands
// BEFORE the operator refuses the pair. An object's ToPrimitive is user
// code, so if it throws, ITS error is the one the program sees.
//
// test262 `unsigned-right-shift/bigint-toprimitive` asserts exactly
// this and caught the first version of band B: `>>> 0n` emitted the
// TypeError up front and the MyError never happened. That case is the
// evidence for the gate (it moved bug -> incompatible when the gate
// landed); a fixture cannot show a refusal, so what it pins here is
// the ordering the gate exists to protect — the moment band B is ever
// let near an operand that can run user code, these lines go wrong.
class MyError extends Error {}

function name_of(f: () => void): string {
  try {
    f();
    return "no throw";
  } catch (e: any) {
    return e.constructor.name;
  }
}

const zero: any = 0n;
const two: any = 2n;
const sym: any = Symbol();

const hostile: any = {
  [Symbol.toPrimitive]: function () {
    throw new MyError("from toPrimitive");
  },
};
const hostileValueOf: any = {
  valueOf: function () {
    throw new MyError("from valueOf");
  },
};

// `>>>` has no BigInt form, so the pair always ends in a TypeError —
// but only after the left side's hook has had its turn.
console.log(name_of(() => { hostile >>> zero; }));
console.log(name_of(() => { hostileValueOf * two; }));
// Symbol refuses ToNumeric, and that refusal is also downstream of the
// left operand's hook.
console.log(name_of(() => { hostileValueOf - sym; }));

// A hook that returns normally still ends in the operator's TypeError.
const quiet: any = { [Symbol.toPrimitive]: function () { return 2n; } };
console.log(name_of(() => { quiet >>> zero; }));

// Both sides primitive: no user code can run, so band B is exact and
// the throw is decided while compiling — this line takes the typed
// spelling on purpose, so it is the gate's ALLOW side and not the any
// kernel answering for it.
const bi: bigint = 1n;
const nu: number = 1;
console.log(name_of(() => { bi & nu; }));
