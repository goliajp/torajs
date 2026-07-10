// chunk 796 — ES §15.5.5 named function expressions keep their
// self-name: `let l = function named() {}` prints [Function: named],
// not [Function: l]. Previously the parser discarded the optional
// name ("accept and discard") so the NamedEvaluation registry fell
// back to the binding name. Anonymous fn expressions still take the
// binding / property-key name (§13.15.2 / §13.2.5.5 control rows).
let l = function named() {
  return 2;
};
console.log(l);
console.log(l());
const k = function () {
  return 3;
};
console.log(k);
console.log(k());
const obj = {
  cb: function tick() {
    return 4;
  },
};
console.log(obj.cb);
console.log(obj.cb());
