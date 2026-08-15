// 392-04 — `arguments` inside a `with` body: an arrow has no
// `arguments` of its own, so the object record answers the name
// first (§9.1.1.2.1 HasBinding per resolution); a function
// EXPRESSION binds its own `arguments` in front of the object, so
// the object never captures it there.
function outer() {
  const o = { arguments: [9, 9] };
  with (o) {
    const f = () => arguments.length;
    return f();
  }
}
console.log(outer(1, 2, 3));
function outer2() {
  const o = { arguments: [7] };
  with (o) {
    const g = function () {
      return arguments.length;
    };
    return g(5, 6);
  }
}
console.log(outer2());
