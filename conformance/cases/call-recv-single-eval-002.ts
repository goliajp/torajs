// A side-effecting receiver expression of a method call must evaluate
// exactly once. `f5(tick())` reaches struct-method-dispatch (the class
// admit sees `next` typed as a Function), which lowers the receiver and
// then declines because `next` is a class METHOD, not a layout field —
// without parking, the sibling-class arm lowered it a second time.
let n = 0;
const it2: any = (function*() {})();
function* f5([] = it2) { }
function tick(): any { n = n + 1; return it2; }
const r = f5(tick()).next();
console.log(n);
console.log(r.done);
