// A "use strict" prologue makes the eval code strict even when the
// function that calls eval is sloppy (§11.2.2), so a refused delete in
// a statement-position eval throws there, and answers false without it.
function setup() {
  var o = { all: 1, noconfig: 1 };
  Object.defineProperty(o, "noconfig", { configurable: false });
  return o;
}
function strictEval() {
  try {
    eval("'use strict'; var o = setup(); delete o.noconfig");
    return "no throw";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
function sloppyEval() {
  return eval("var o = setup(); delete o.noconfig");
}
console.log(strictEval());
console.log(sloppyEval());
var arrow = () => {
  try {
    eval("'use strict'; var o = setup(); delete o.noconfig");
    return "no throw";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
};
console.log(arrow());
// the strict eval's own var stays inside it, and `this` is the caller's
var host = {
  tag: "host",
  run: function () {
    eval("'use strict'; var inner = this.tag; console.log(inner)");
    return typeof inner;
  },
};
console.log(host.run());
