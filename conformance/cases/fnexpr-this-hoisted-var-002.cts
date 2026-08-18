// Sloppy (script-goal) half of the hoisted-var promote: a plain call's
// `this` answers globalThis (§10.2.1.2 + the sloppy prologue), and the
// binding is deliberately named `w` — the with-helper's params used to
// be spelled `w`/`k`, and splicing them into the user arena made any
// user binding of the same name lose its by-name promotion (rotation
// 437, the `__twith_` rename pins that).
try {
  var f = function () { return this === globalThis; };
} catch (e) {}
console.log(f());

var myObj: any = { p1: 5 };
var p1: any = "outer";
var st: any = 0;
try {
  var w = function () {
    with (myObj) {
      st = p1;
      this.p2 = 88;
    }
  };
  w();
} catch (e) {}
console.log(st, myObj.p1);
