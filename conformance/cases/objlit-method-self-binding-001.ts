// An object literal whose method refers to the literal's OWN binding
// (`var t = { then() { … t … } }`, the test262 thenable idiom) must
// not be typed nominally: a nominal literal's methods lift to
// receiver-only bodies with no closure env, so the captured name had
// nowhere to live and the body died at "unknown identifier". Such a
// literal takes the any lane, whose methods are ordinary closures
// (rotation 650). Any binding kind; a method that captures nothing
// stays nominal.
var u = { x: 1, m: function () { return u.x; } };
console.log("var", u.m());
const c = { x: 2, m: function () { return c.x; } };
console.log("const", c.m());
let l = { x: 3, m: function () { return l.x + 1; }, n: 5 };
console.log("let", l.m(), l.n);
var v = { x: 4, m: function () { var k = 1; return k; } };
console.log("no-selfref", v.m());
var seq: number[] = [];
var thenable = { then: function (this: any, onResolve: any, onReject: any) { seq.push(3); console.log("then", this === thenable, seq.join(",")); onResolve("resolved"); throw new Error("interrupt flow"); } };
seq.push(1);
var p1 = Promise.resolve(thenable);
seq.push(2);
p1.then(function (q: any) { seq.push(5); console.log("all done", q, seq.join(",")); });
function f() { var w = { x: 6, m: function () { return w.x * 2; } }; return w.m(); }
console.log("nested", f());
