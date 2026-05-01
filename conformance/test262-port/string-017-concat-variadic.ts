// Adapted from test262: built-ins/String/prototype/concat/* — the
// variadic counterpart of `+`. tr lowers `s.concat(a, b, c)` as a left-
// fold over the existing str_concat intrinsic so the cost is one alloc
// per pair, identical to the operator chain `((s + a) + b) + c`.
function check(): number {
  // Single-arg.
  if ("hello".concat(" world") !== "hello world") { throw "#1: single"; }
  if ("".concat("x") !== "x") { throw "#2: empty receiver"; }

  // Two-arg.
  if ("a".concat("b", "c") !== "abc") { throw "#3: two-arg"; }

  // Variadic — three+ args.
  if ("[".concat("1", ",", "2", ",", "3", "]") !== "[1,2,3]") {
    throw "#4: variadic";
  }

  // Empty arg list — receiver returned unchanged.
  if ("preserve me".concat() !== "preserve me") { throw "#5: empty args"; }

  // Empty strings mid-chain don't change the result.
  if ("ab".concat("", "cd", "", "ef") !== "abcdef") { throw "#6: empties"; }

  // Identity: s.concat(rest) === s + rest.
  let s = "test";
  let plus = s + "ing";
  let cc = s.concat("ing");
  if (plus !== cc) { throw "#7: equiv to +"; }

  // Identifier receiver — exercise non-literal recv_op path.
  let prefix = "Mr.";
  let space = " ";
  let last = "Smith";
  if (prefix.concat(space, last) !== "Mr. Smith") { throw "#8: ident recv"; }

  return 0;
}
console.log(check());
