// Phase M6.3 — JSON.stringify for Array / Object, including nested
// shapes. The recursive walker (`lower_json_stringify` in ssa_lower)
// dispatches at compile time on the static type of each value:
//
//   Array<T>            → runtime loop with alloca'd accumulator;
//                          each element recursively serialized,
//                          comma-separated, wrapped with `[ ]`
//   { f1: T, f2: U, … } → unrolled per-field at compile time;
//                          field name interned, `__torajs_json_quote_str`
//                          escapes it, value recursively serialized,
//                          colon-joined, wrapped with `{ }`
//
// Nesting works automatically (Array<Object>, Object with Array
// fields, etc.) since the recursive call dispatches on each
// element / field's declared static type.

type Point = { x: number, y: number };
type User = { name: string, age: number, active: boolean };

function check(): number {
  let p: Point = { x: 3, y: 4 };
  if (JSON.stringify(p) !== "{\"x\":3,\"y\":4}") {
    throw "#1: simple object";
  }

  let u: User = { name: "alice", age: 30, active: true };
  if (JSON.stringify(u) !== "{\"name\":\"alice\",\"age\":30,\"active\":true}") {
    throw "#2: object with mixed-type fields";
  }

  let nums: number[] = [1, 2, 3];
  if (JSON.stringify(nums) !== "[1,2,3]") { throw "#3: number array"; }

  let strs: string[] = ["a", "b\"c"];
  if (JSON.stringify(strs) !== "[\"a\",\"b\\\"c\"]") {
    throw "#4: string array with escape";
  }

  let bools: boolean[] = [true, false, true];
  if (JSON.stringify(bools) !== "[true,false,true]") {
    throw "#5: boolean array";
  }

  // Empty array / single-element array.
  let empty: number[] = [];
  if (JSON.stringify(empty) !== "[]") { throw "#6: empty array"; }

  let one: number[] = [42];
  if (JSON.stringify(one) !== "[42]") { throw "#7: single-element array"; }

  // Nested: array of objects.
  let pts: Point[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];
  if (JSON.stringify(pts) !== "[{\"x\":1,\"y\":2},{\"x\":3,\"y\":4}]") {
    throw "#8: array of objects";
  }

  return 0;
}
console.log(check());
