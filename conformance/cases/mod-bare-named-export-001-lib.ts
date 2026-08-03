// P13-S4b lib — bare named export (`export { a, b as c }`): the
// decls stay ordinary top-level statements; the export FACE lists
// them (rename included) after the fact.
var endTime = 42;
function f() {
  return 7;
}
let hidden = 1;
export { endTime as time, f };
