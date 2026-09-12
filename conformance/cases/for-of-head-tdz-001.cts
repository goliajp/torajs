// rotation 638 — §14.7.5.6 ForIn/OfHeadEvaluation step 2: a let / const head's
// names are in their dead zone while the source is evaluated, and a function
// written in the source captures that never-initialized environment.
// oracle: node script goal.
function t1() { let x = 1; try { for (let x in { x }) {} return "no throw"; } catch (e) { return e instanceof ReferenceError ? "ReferenceError" : String(e); } }
function t2() { let x = 1; try { for (const x of [x]) {} return "no throw"; } catch (e) { return e instanceof ReferenceError ? "ReferenceError" : String(e); } }
let x = "outside";
var probeBefore = function () { return x; };
var probeExpr;
for (let x in { i: probeExpr = function () { typeof x; } }) ;
var r3 = probeBefore();
var r4; try { probeExpr(); r4 = "no throw"; } catch (e) { r4 = e instanceof ReferenceError ? "ReferenceError" : String(e); }
var ys = [1, 2]; var out = [];
for (let y of ys) { out.push(y); }
let z = 5; var zz = [];
for (let w of [z]) { zz.push(w); }
console.log(t1(), t2(), r3, r4, out.join(","), zz.join(","));
