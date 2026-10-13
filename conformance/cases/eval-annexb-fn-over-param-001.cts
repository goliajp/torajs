// Annex B §B.3.2.3: a block-nested function declared by sloppy direct
// eval text inside a function writes the function's binding of that
// name when the block runs, even when the name is a parameter (unlike
// §B.3.3.1 for the function's own blocks, which skips parameter names).
//
// Oracle is node's real script goal, pinned in `.expected`.
var init: any, after: any;
(function (f: any) {
  eval("init = f; { function f() {} } after = f;");
})(123);
console.log(init, typeof after);
var init2: any, after2: any;
(function (g: any) {
  eval("init2 = g; if (true) function g() { return 7; } after2 = g;");
})("s");
console.log(init2, typeof after2, after2());
function own(h: any) {
  { function h() {} }
  return h;
}
console.log(own(5));
