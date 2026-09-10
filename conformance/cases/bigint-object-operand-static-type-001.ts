// §13.7-§13.10 / §13.5.5 — an object operand beside a BigInt, or under
// unary minus, may answer a BigInt through ToNumeric, so the static
// type is the any lane's: a generic helper called with such a result
// and a BigInt literal infers one T (the test262 assert.sameValue
// shape over expressions/{division,modulus,multiplication,subtraction}
// /bigint-toprimitive and unary-minus/bigint-non-primitive).
function same<T>(actual: T, expected: T, msg: string) {
  console.log(msg, actual === expected, typeof actual);
}
function err() { throw new Error("no"); }
const two = { [Symbol.toPrimitive]: function () { return 2n; }, valueOf: err, toString: err };
same(two / 2n, 1n, "div");
same(2n / two, 1n, "div-r");
same(two * 2n, 4n, "mul");
same(two - 2n, 0n, "sub");
same(7n % two, 1n, "mod");
same(-Object(1n), -1n, "neg-wrapper");
same(-{ valueOf() { return 5n; } }, -5n, "neg-valueof");
same(-Object(3), -3, "neg-number-wrapper");
console.log((() => { try { return two * 2; } catch (e) { return (e as Error).constructor.name; } })());
