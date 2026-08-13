// §14.11 — the parser writes references to globals as MACHINERY, and
// a `with` object must not answer those.
//
// `${v}` in a template becomes `String(v)`, and a for-in head becomes
// `Object.__forinKeys(src)`. Both spell a name this object carries, so
// both were resolved through it: the template answered "HIJACKED"
// silently, and the for-in either answered the hijack or — in the
// ordinary case where the object carries nothing — destroyed the shape
// its lowering recognises and threw. §13.2.8.6 and §14.7.5 both name
// abstract operations, not lookups of a binding.
//
// The two calls the USER writes are the control: those really are name
// resolutions, and the object really does answer them. Without that
// half this file would pass just as well if the exemption were too
// wide.
//
// `.cts` because `with` only exists under the sloppy goal.

var v: any = 5;
var src: any = { a: 1, b: 2 };

var o: any = {
  String: function (x: any): any {
    return "HIJACKED-String";
  },
  Object: {
    __forinKeys: function (x: any): any {
      return ["HIJACKED-keys"];
    },
    keys: function (x: any): any {
      return ["USER-keys"];
    },
  },
};

with (o) {
  // Machinery: the object must not be consulted.
  console.log(`v=${v}`);
  for (var k in src) {
    console.log("forin", k);
  }

  // Written by the program: the object answers, because these really
  // are the name resolutions §9.1.1.2.1 governs.
  console.log(String(v));
  console.log(Object.keys(src));
}
