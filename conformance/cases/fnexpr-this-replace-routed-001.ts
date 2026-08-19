// knife-2 variable-routed replacer in the replace cb slot — the
// binding's only use is the callback position, so it rides the
// zero-alias profile; the replace_fn kernel reads the receiver-first
// flag and seeds `this` undefined (§22.1.3.18 step 10).
var calls: any = [];
var replaceValue = function (...args: any[]) {
  calls.push(this);
  return "X";
};
console.log("abcab".replaceAll("ab", replaceValue));
console.log(calls.length, calls[0] === undefined, calls[1] === undefined);

var one = function () { return "Y"; };
console.log("q-q".replace("-", one));
