// A machine local holding a closure with a REST parameter is typed
// `__rest(E[])` in the flat annotation — the field used to take the
// declared `any[]` and the assignment failed check ("field is
// Function([Array(Any)]), value is Function([Rest(Any)])"), which the
// `Array/fromAsync/this-constructor` cases hit once their nested
// `function MyArray(...args)` became a value. (Calling such a closure
// directly inside the machine — `sum(1, 2)` on a `(...xs) => …` field —
// is a separate, registered gap.)
async function declShape() {
  const calls: any[] = [];
  function MyArray(...args: any[]) { calls.push(args); }
  let result: any = await Array.fromAsync.call(MyArray, [1, 2]);
  console.log("decl", result instanceof MyArray, result.length, calls.length, JSON.stringify(calls[0]));
}
async function exprShape() {
  const calls: any[] = [];
  const MyArray = function (...args: any[]) { calls.push(args); };
  let result: any = await Array.fromAsync.call(MyArray, { length: 2, 0: 1, 1: 2 });
  console.log("expr", result instanceof MyArray, result.length, calls.length, JSON.stringify(calls[0]));
}
async function main() { await declShape(); await exprShape(); }
main();
