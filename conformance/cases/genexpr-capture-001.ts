// Generator expressions capturing enclosing locals — the wrapper
// route (RFC 20260713 residual): captured names ride as prepended
// factory params behind an arrow wrapper, so the factory body
// compiles through the existing params->this generator prep.
function make(base: any): any {
  var step: any = 10;
  return function* (n: any) {
    yield base + n;
    yield step + n;
  };
}
var g: any = make(100);
var it: any = g(1);
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next().done);
var it2: any = make(200)(2);
console.log(it2.next().value);
console.log(it2.next().value);
var y: any = 5;
var h: any = function* () {
  yield y;
};
console.log(h().next().value);
function outer2(): any {
  var q: any = 3;
  var ag: any = async function* () {
    yield q;
    yield q + 1;
  };
  return ag;
}
var agen: any = outer2()();
agen.next().then(function (r: any) {
  console.log("async", r.value, r.done);
});
