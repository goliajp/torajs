// A call whose checker type is `void` answers `undefined`. Its lowering
// may hand back a placeholder operand of another SSA type, so boxing by
// the OPERAND's type encoded the placeholder instead of undefined —
// §25.5.2 serializes an undefined array element as "null", and tr
// answered "0".
//
// This closes the paths that box through the expression (which knows the
// checker's type). A typed / any ARRAY's `forEach` still leaks its
// `ConstI64(0)` placeholder when the call is written inline as an
// element or argument, because those sites box by operand type; the
// root fix is a Void constant operand, registered separately.
function nothing(): void {}
console.log(JSON.stringify([nothing()]));
console.log(JSON.stringify({ k: nothing() }));
const arr: any[] = [nothing(), 1, nothing()];
console.log(arr.length, typeof arr[0], arr[0] === undefined, JSON.stringify(arr));

const o: any = { 0: 1, length: 1 };
console.log(JSON.stringify([(Array.prototype.forEach as any).call(o, function () {})]));

// Binding the call first goes through the same route on every receiver.
const a: number[] = [1, 2, 3];
const x: any = a.forEach(function () {});
console.log(typeof x, x === undefined, JSON.stringify([x]));

// An explicit `undefined` and a `null` stay distinct.
console.log(JSON.stringify([undefined, null, 0]));
console.log(String(nothing()), `${nothing()}`);
