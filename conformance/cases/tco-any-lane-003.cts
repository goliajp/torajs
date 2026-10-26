// A sloppy script: tail position is defined only for strict code
// (sec-isintailposition step 1), so a call through an `any` callee
// in a sloppy function keeps the ordinary call, while a function
// whose body says "use strict" takes the tail-call form. The depth
// stays small; the results must match either way.
var sloppy: any;
sloppy = function (n) {
  if (n === 0) return "sloppy done";
  return sloppy(n - 1);
};
console.log(sloppy(1000));

var strict: any;
strict = function (n) {
  "use strict";
  if (n === 0) return "strict done";
  return strict(n - 1);
};
console.log(strict(1000));

var mixed: any;
mixed = function (n) {
  if (n === 0) return "mixed done";
  return strictHop(n - 1);
};
function strictHop(n) {
  "use strict";
  return mixed(n);
}
console.log(mixed(1000));
