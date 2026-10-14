// Annex B §B.3.3 does not exist for strict code: a block function in eval
// text whose own prologue says "use strict" binds its block only, direct
// or indirect, even inside a sloppy script.
//
// Oracle is node's real script goal, pinned in `.expected`.
eval('"use strict";{ function sf1() {} }');
try { sf1; console.log("direct-block leaked"); } catch (e) { console.log("direct-block", (e as any).constructor === ReferenceError); }
(0, eval)('"use strict";{ function sf2() {} }');
try { sf2; console.log("indirect-block leaked"); } catch (e) { console.log("indirect-block", (e as any).constructor === ReferenceError); }
(0, eval)('"use strict"; switch (1) { case 1: function sf3() {} }');
try { sf3; console.log("indirect-switch leaked"); } catch (e) { console.log("indirect-switch", (e as any).constructor === ReferenceError); }
(0, eval)("{ function lf4() { return 4; } }");
console.log("sloppy-indirect", typeof lf4);
