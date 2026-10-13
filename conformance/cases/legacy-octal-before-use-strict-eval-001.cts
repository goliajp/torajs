// Annex B §B.1.2 with §11.2.1: a legacy octal escape is a SyntaxError in
// strict code, and a "use strict" later in the same directive prologue
// makes the earlier literals strict code too. Eval text is judged by its
// own prologue and by the bodies it declares, never by the file's.
//
// Oracle is node's real script goal, pinned in `.expected`.
try { eval(" '\\145'; 'use strict'; "); console.log("top: ok"); } catch (e) { console.log("top:", (e as any).name); }
try { eval(" function f(){ '\\145'; 'use strict'; } "); console.log("fn: ok"); } catch (e) { console.log("fn:", (e as any).name); }
try { eval(" 'use strict'; var s = '\\101'; "); console.log("after: ok"); } catch (e) { console.log("after:", (e as any).name); }
try { eval(" '\\8'; 'use strict'; "); console.log("nonoctal: ok"); } catch (e) { console.log("nonoctal:", (e as any).name); }
try { (0, eval)(" '\\145'; 'use strict'; "); console.log("indirect: ok"); } catch (e) { console.log("indirect:", (e as any).name); }
try { console.log("sloppy:", eval("5; '\\101'; function g() { 'use strict'; } '\\102'")); } catch (e) { console.log("sloppy:", (e as any).name); }
function notAnError(): string {
  5;
  "\145";
  function g() { "use strict"; }
  return typeof g;
}
console.log(notAnError(), "\101" + "\102");
