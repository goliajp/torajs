// Strictness has a second source below the program (§11.2.2): a
// `"use strict"` directive makes one function strict inside a
// sloppy Script, and class code is always strict. The [[Set]] and
// delete flavors are picked per FUNCTION now — keyed on the program
// alone, `() => { "use strict"; o[0] = 1 }` and a class method's
// `super.x = v` took the soft kernel here (nine test262 cases).
// `.expected` because bun's CommonJS transpile drops a
// function-level `"use strict"` directive in a .cts (its arrow and
// `nested` cases print "no throw"); node on the same source as .js,
// and test262 (sm/object/bug-1150906), are the oracle behind the file.
const o: any = Object.freeze({ a: 1, 0: 0 });
o.a = 2; o[0] = 5;
console.log(o.a, o[0]);
try { (() => { "use strict"; o.a = 3; })(); console.log("no throw a"); } catch (e: any) { console.log("caught a", e instanceof TypeError); }
try { (function () { "use strict"; o[0] = 6; })(); console.log("no throw 0"); } catch (e: any) { console.log("caught 0", e instanceof TypeError); }
try { (() => { "use strict"; delete o.a; })(); console.log("no throw delete"); } catch (e: any) { console.log("caught delete", e instanceof TypeError); }
console.log(delete o.a, o.a);
class C { m(t: any) { t.a = 9; return "set ran"; } static s(t: any) { delete t.a; return "delete ran"; } }
try { console.log(new C().m(o)); } catch (e: any) { console.log("caught class", e instanceof TypeError); }
try { console.log(C.s(o)); } catch (e: any) { console.log("caught class delete", e instanceof TypeError); }
function sloppy(t: any) { t.a = 10; return t.a; }
console.log(sloppy(o));
function nested() { "use strict"; return (function () { try { o.a = 11; return "no throw"; } catch (e: any) { return "caught inherited"; } })(); }
console.log(nested());
