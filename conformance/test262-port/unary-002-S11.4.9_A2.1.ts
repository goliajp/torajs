// Adapted from test262: language/expressions/logical-not/S11.4.9_A2.1_T*.js
// Restricted to boolean operands (we don't do the spec's ToBoolean coercion
// from arbitrary values yet).
function check(): number {
  if (!true !== false) { throw "#1: !true"; }
  if (!false !== true) { throw "#2: !false"; }
  let b: boolean = true;
  if (!b !== false) { throw "#3: !b"; }
  if (!!b !== true) { throw "#4: !!b"; }
  return 0;
}
console.log(check());
