// §11.2.1: a Use Strict Directive is the exact code point sequence
// "use strict" or 'use strict' — a literal spelled with an escape or a
// line continuation has the same cooked value but stays an ordinary
// directive, so the function remains sloppy. A real one later in the
// same prologue still makes the body strict.
//
// Oracle is node's real script goal, pinned in `.expected`.
function esc(): boolean {
  "use\u0020strict";
  return this !== undefined;
}
function cont(): boolean {
  'use str\
ict';
  return this !== undefined;
}
function hex(): string {
  "use\x20strict";
  try {
    undeclared31 = 1;
    return "sloppy";
  } catch (e) {
    return (e as any).name;
  }
}
function later(): boolean {
  "use\x20strict";
  "use strict";
  return this === undefined;
}
function plain(): boolean {
  "use strict";
  return this === undefined;
}
const arrowEsc = (a = 1): boolean => {
  "use\u0020strict";
  return a === 1;
};
console.log(esc(), cont(), hex(), later(), plain(), arrowEsc());
console.log(eval('"use\\x20strict"'));
