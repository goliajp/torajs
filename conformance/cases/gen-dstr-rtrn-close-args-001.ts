// rotation 455 — the t262 rtrn-close BASIC variant shape: the
// iterator duo ({ next: fn, return: fn }) touches `arguments` and
// `this` inside its methods, and the only member spelling of those
// names is off a GENERATOR INSTANCE binding (it.next / it.return),
// which must not refuse the objlit boxed-only argv admission.
let args: any = null;
let thisValue: any = null;
let returnCount = 0;
const iterator = {
  next: function() { return { done: false, value: undefined }; },
  return: function() {
    returnCount += 1;
    thisValue = this;
    args = arguments;
    return {};
  }
};
const iterable: any = {};
iterable[Symbol.iterator] = function() { return iterator; };
function* g() {
  let x: any;
  [ x = yield ] = iterable;
}
const it = g();
it.next();
it.return(777);
console.log("rc", returnCount, "args.length", args.length, "this-ok", thisValue === iterator);
