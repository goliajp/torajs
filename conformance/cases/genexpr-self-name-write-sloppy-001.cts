// §15.5.5 — a write to a generator expression's own self-name is ignored
// in sloppy code; the name still answers the function itself.
let callCount = 0;
let ref = function * BindingIdentifier() {
  callCount++;
  BindingIdentifier = 1;
  return BindingIdentifier;
};
console.log(ref().next().value === ref, callCount);
let ref2 = function * B2() {
  (() => { B2 = 1; })();
  return B2;
};
console.log(ref2().next().value === ref2);
let ref3 = function * B3() {
  B3++;
  return B3;
};
console.log(ref3().next().value === ref3);
