// An imported binding named like a global function property is the
// module-environment binding a bare call reaches (§9.1.1.5, §9.1.1.1).

import { parseInt, escape } from "./binding-shadows-global-fn-003-lib.ts";

console.log(parseInt("12"), escape(5));
console.log(unescape("%41"), isNaN(Number("x")));
