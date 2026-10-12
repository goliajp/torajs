// A top-level fn-typed binding initialized by a closure-returning call is
// in its temporal dead zone until the declaration runs: a named function
// that calls or reads it earlier throws a ReferenceError.
function mk(k: number): (y?: number) => number { return (y?: number) => k + (y ?? 0) }
function u() { return g() }
function v() { return g }
function w() { return g(2) }
try { u() } catch (e) { console.log("u", e instanceof ReferenceError) }
try { v() } catch (e) { console.log("v", e instanceof ReferenceError) }
try { w() } catch (e) { console.log("w", e instanceof ReferenceError) }
const g: (y?: number) => number = mk(1)
console.log(u(), w(), typeof v())
