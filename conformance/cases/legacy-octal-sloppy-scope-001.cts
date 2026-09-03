// The sloppy-goal side of the Annex B legacy-octal gate (rotation 574):
// an explicit `"use strict"` prologue makes ITS body strict code and
// nothing else, so these all keep their sloppy values.
// No directive at all — an ordinary sloppy function.
function plain() { return "\101" }
console.log(plain(), 010);
// A directive that is not `"use strict"` arms nothing.
function other() { "not strict"; return 010 }
console.log(other());
// A strict function's sibling code is untouched by it.
function strict1() { "use strict"; return 1 }
console.log(strict1(), "\101", 010);
// And a string that merely CONTAINS the words is an ordinary value.
console.log("use strict".length, `\x30`);
