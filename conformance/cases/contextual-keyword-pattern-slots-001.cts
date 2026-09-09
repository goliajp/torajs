// Every slot of a destructuring pattern is a binding position, and
// they used to disagree about it: the object shorthand slot asked
// `shorthand_binding_name`, while array elements, both rest slots and
// the rename targets matched `Ident` alone. So `var { let }` bound and
// `var [ let ]`, `var { ...let }`, `var { a: let }` were SyntaxErrors
// that node runs. Script goal — a module reserves all of these, and
// that face is an early error in the negative suite.
var [ pLet ] = [1];
var [ let ] = [2];
console.log(let);
var [ yield ] = [3];
console.log(yield);
var [ async ] = [4];
console.log(async);

var { ...restLet } = { a: 5 };
console.log(restLet.a);
var [ ...arrLet ] = [6, 7];
console.log(arrLet.length);

var { k: let2 } = { k: 8 };
console.log(let2, pLet);

function fa([ let ]) { return let; }
console.log(fa([9]));
function fo({ ...let }) { return let.z; }
console.log(fo({ z: 10 }));
function fr([ ...let ]) { return let[0]; }
console.log(fr([11]));
function fn2({ k: yield }) { return yield; }
console.log(fn2({ k: 12 }));

for (var [ let ] of [[13]]) { console.log(let); }
for (var { ...let } of [{ w: 14 }]) { console.log(let.w); }

var { c: [ let ] } = { c: [15] };
console.log(let);

var [ let = 16 ] = [];
console.log(let);
