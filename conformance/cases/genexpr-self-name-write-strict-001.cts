"use strict";
// §15.5.5 — in strict code the same write throws TypeError, after the
// RHS is evaluated.
let callCount = 0;
let ref = function * BindingIdentifier() {
  callCount++;
  BindingIdentifier = 1;
  return BindingIdentifier;
};
try { ref().next(); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError, e.message); }
console.log(callCount);
let ref2 = function * B2() {
  (() => { B2 = 1; })();
  return B2;
};
try { ref2().next(); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }
