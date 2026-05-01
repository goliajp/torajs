// Adapted from test262: language/expressions/typeof/* — `typeof x`
// yields a string literal at runtime, resolved at codegen from the
// operand's static type.
type Pt = { x: number };

function check(): number {
  let n: number = 42;
  let s: string = "hi";
  let b: boolean = true;
  let arr: number[] = [1, 2, 3];
  let p: Pt = { x: 5 };

  if (typeof n !== "number") { throw "#1"; }
  if (typeof s !== "string") { throw "#2"; }
  if (typeof b !== "boolean") { throw "#3"; }
  if (typeof arr !== "object") { throw "#4"; }
  if (typeof p !== "object") { throw "#5"; }
  // Literal forms
  if (typeof 1 !== "number") { throw "#6"; }
  if (typeof "abc" !== "string") { throw "#7"; }
  if (typeof false !== "boolean") { throw "#8"; }
  return 0;
}
console.log(check());
