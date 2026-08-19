// §9.1.1.4.6 — sloppy assignment to a never-declared name creates the
// binding; before the write runs it reads as undefined / typeof
// "undefined" (the hoisted-var shape of the implicit global).
__ig1 = 41;
console.log(__ig1, typeof __ig1);
function f() {
  __ig2 = "x";
}
f();
console.log(__ig2);
try {
  while (
    (function () {
      throw 1;
    })()
  )
    __never = "reached";
} catch (e) {
  console.log(e, typeof __never);
}
__later = __ig1 + 1;
console.log(__later);
