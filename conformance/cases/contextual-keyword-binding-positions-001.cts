// `async` and `type` are on nobody's reserved list: `async` is a keyword
// only where the grammar looks ahead for it, `type` only in TypeScript's
// declaration position. Rotation 618 admitted them as property
// shorthands and as IdentifierReferences; this covers the third group,
// the BINDING positions, which refused all ten of these.
//
// node runs every line. Measured before the cut, tr rejected all ten.

var v1 = 0;
var async = 1;
let asyncLet = 2;
const type = 3;
console.log(async, asyncLet, type, v1);

// `let async = x` needed one more thing than the others: at statement
// start `let` has two readings, and the token after it decides. `async`
// was not on the list that makes it a LexicalDeclaration, so the line
// read as the name `let` followed by a stray `async`.
let async2 = 4;
console.log(async2);

function param(async, type) { return async + "/" + type; }
console.log(param(5, 6));

const arrow = (async) => async;
console.log(arrow(7));

try { throw 8; } catch (async) { console.log(async); }

function type2() { return 9; }
console.log(type2());

for (var async3 of [10]) console.log(async3);
for (const type3 of [11]) console.log(type3);

// The forms that make `async` a keyword still parse as keywords.
async function af() { return 12; }
const g = async () => 13;
const obj = { async m() { return 14; }, type: 15 };
console.log(obj.type);
af().then((a) => g().then((b) => obj.m().then((c) => console.log(a, b, c))));
