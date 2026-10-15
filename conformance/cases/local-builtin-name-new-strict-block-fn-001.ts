// In strict code a function declared in a block binds in that block
// only (§14.2.2; Annex B §B.3.3 applies to sloppy code), so outside the
// block `new` reaches the builtin.

class A {
  m() {
    {
      function TypeError() {}
    }
    return new TypeError("t");
  }
}

function strictFn() {
  "use strict";
  if (true) {
    function RangeError() {}
  }
  return new RangeError("r");
}

const t = new A().m();
console.log(t instanceof TypeError, t.message);
const r = strictFn();
console.log(r instanceof RangeError, r.message);
