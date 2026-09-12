// RFC 20260912-global-var-cell knife 4-2 — a `var` declared only by eval /
// Script text binds its global-object property WHERE THE TEXT RAN, with
// the text's attributes: an eval's is deletable (§19.2.1.3 D = true), a
// Script's is not (§16.1.7). A binding with no bare compiled writer is
// "writerless": the object's attributes are the whole truth about writes,
// so a `var` over a pre-existing read-only property keeps it (§16.1.7
// CanDeclareGlobalVar: do nothing) and `writable: false` can be honoured.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`.
// Registered residue, not in this fixture: `writable: false` over a `var` the
// program WRITES stays refused (the compiled bare writes could not honour it).
function desc(n) {
  var d = Object.getOwnPropertyDescriptor(this, n);
  return d === undefined ? "absent" : [d.value, d.writable, d.enumerable, d.configurable].join(",");
}
console.log("before", "ee" in globalThis, desc("ee"));
(0, eval)("var ee;");
console.log("eval-var", typeof ee, "ee" in globalThis, desc("ee"));
console.log("delete", delete globalThis.ee, "ee" in globalThis);
__t262_evalScript("var sv = 5;");
console.log("script-var", sv, desc("sv"), delete globalThis.sv, "sv" in globalThis);
// declared through the object first, then by eval text: the property is kept
Object.defineProperty(this, "cc", { configurable: true, writable: true, enumerable: true, value: 5 });
(0, eval)("var cc;");
console.log("kept", cc, globalThis.cc, desc("cc"));
Object.defineProperty(this, "ro", { configurable: true, writable: false, enumerable: false, value: 0 });
Object.defineProperty(this, "nc", { configurable: false, writable: false, enumerable: false, value: 1 });
Object.preventExtensions(this);
__t262_evalScript("var ro;");
__t262_evalScript("var nc;");
console.log("read-only kept", ro, nc, desc("ro"), desc("nc"));
this.ro = "written?";
this.nc = "written?";
console.log("still", ro, nc, globalThis.ro, globalThis.nc);
// a program `var` nobody writes can become read-only through the object
var wl;
Object.defineProperty(this, "wl", { writable: false });
this.wl = 9;
console.log("writerless", wl, desc("wl"));
