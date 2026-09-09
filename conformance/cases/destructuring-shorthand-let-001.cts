// `let` shorthands in a destructuring pattern wherever the BoundNames
// are not a lexical declaration's. It is the fourth contextual word and
// the only one that needs the BINDING FORM as well as the strictness:
// §14.3.1.1 refuses `let` in the BoundNames of a `let` or `const` in
// sloppy code too, which the other three are not subject to.
//
// Measured against node, all eleven cells matching:
//   bind    var { let } · function f({ let }) · ({ let }) => · nested ·
//           for (var { let } of …) · { let: x } rename
//   reject  let { let } · const { let } · for (let { let } of …) ·
//           catch ({ let }) · "use strict"
//
// catch is the cell that shows the axis is lexical-vs-var rather than
// var-vs-everything: `catch ({ yield })` binds and `catch ({ let })`
// does not.

var o = { let: 5 };
var { let } = o;
console.log(let);

function param({ let }) { return let; }
console.log(param({ let: 6 }));

const arrow = ({ let }) => let;
console.log(arrow({ let: 7 }));

for (var { let } of [{ let: 9 }]) console.log(let);

var nested = { a: { let: 11 } };
var { a: { let } } = nested;
console.log(let);

// The rename path, which is how this word reached a pattern before it
// could shorthand at all.
var { let: renamed } = { let: 12 };
console.log(renamed);

// A default on the shorthand. Kept to ONE field deliberately: a
// multi-field parameter pattern drops the first field's default
// entirely — `function e({ q = 5, ...rest })` answers undefined, with
// an ordinary identifier and no contextual keyword anywhere near it —
// which is a separate, pre-existing defect this fixture must not
// smuggle in as if it were about `let`.
function defaulted({ let = 13 }) { return let; }
console.log(defaulted({}));
console.log(defaulted({ let: 14 }));
