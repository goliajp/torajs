// §13.4.4 over the global object: `for (i = 0; …; i++)` creates `i`
// in the first clause and UPDATES it in the third, so the update's
// §13.4.4.1 step 1 GetValue has to consult the object half of the
// record the same way a bare read does.
//
// And strictness is per-BODY (§11.2.2), not per-program: a
// `"use strict"` function inside a sloppy Script keeps §6.2.5.6's
// ReferenceError for a write it cannot resolve.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS.
for (index8 = 0; index8 < 3; index8++) {}
console.log(index8);
c9 = 1;
console.log(c9++, c9, ++c9, c9, c9--, c9);
function strictOne(): string {
  "use strict";
  try {
    nope7 = 1;
  } catch (e) {
    return (e as any).name;
  }
  return "no throw";
}
console.log(strictOne());
