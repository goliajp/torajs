// An object literal field initialized from a CAPTURED binding takes
// its own share: the capture preamble's binding is env-owned
// (borrowed), so the field store must inc — bare-storing let the
// struct drop release the env's only stake and the promise died
// after one mint (its defineProperty face read back scrubbed).
function f() {
  var q = new Promise(function () {});
  Object.defineProperty(q, "then", {
    value: function () {
      console.log("t");
    },
  });
  function mk(): any {
    return { done: false, value: q };
  }
  mk();
  mk();
  (q as any).then(
    function () {},
    function () {}
  );
  console.log("done");
}
f();
