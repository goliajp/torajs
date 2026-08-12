// string-pattern replace/replaceAll functional replaceValue over a
// string-literal receiver: the replacer runs with this = undefined
// per §22.1.3.18 step 10 Call(replaceValue, undefined, ...) — the
// sloppy goal then binds globalThis (ThisMode ~global~).
var x: any = 3;
var r = "ab".replace("b", function () {
  x = this;
  return "c";
});
console.log(r, x === globalThis);
var z: any = 0;
var r3 = "aba".replaceAll("a", function (m: any, pos: any) {
  z = this;
  return "_" + pos;
});
console.log(r3, z === globalThis);
