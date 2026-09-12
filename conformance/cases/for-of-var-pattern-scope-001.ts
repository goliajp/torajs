// Rotation 636 — a `var` destructuring head in for-of / for-in binds
// function-scoped `var`s (§14.7.5.7 varBinding): the binding is
// readable after the loop, shares one slot across iterations, and a
// same-name outer `var` is the same binding. `let` heads stay block
// scoped.
for (var [a] of [[13], [14]]) console.log("a", a);
console.log("after a", a);
for (var { w } of [{ w: 1 }, { w: 2 }]) console.log("w", w);
console.log("after w", w);
var [b] = [2];
for (var [b] of [[15]]) console.log("b", b);
console.log("after b", b);
function f() {
  for (var [p, { q }] of [[1, { q: 2 }], [3, { q: 4 }]]) console.log("pq", p, q);
  return [p, q];
}
console.log("f", f());
for (var [k] in { x: 1 }) console.log("k", k);
console.log("after k", k);
for (let [c] of [[7]]) console.log("c", c);
console.log("c scoped", typeof c);
