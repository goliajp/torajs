// `typeof <bare name>` answered from a table keyed by the name alone,
// so a builtin's name kept answering for the builtin even after the
// program declared something else under it. Nothing about this is
// specific to any one name — `eval` is just where it was noticed, and
// `parseInt` reads exactly the same way.
//
// Both directions belong here: the table must stop answering for a
// name the program took, and must keep answering for every name it
// did not.
var eval = 10;
console.log(typeof eval);

var parseInt = 3;
console.log(typeof parseInt);

let Boolean2 = 1;
console.log(typeof Boolean2);

// untouched names still resolve to the builtin
console.log(typeof Math, typeof JSON, typeof Reflect);
console.log(typeof Array, typeof Promise, typeof isNaN, typeof Boolean);
console.log(typeof undefined);

// a name nothing declares at all is still `undefined`, not a guess
console.log(typeof neverDeclaredAnywhere);

// user declarations of ordinary names keep their own answers
function ordinary() {}
console.log(typeof ordinary);

class Widget {}
console.log(typeof Widget);

// shadowing inside a function body, where the local is what shows
function inner(): string {
  const Symbol = "taken";
  return typeof Symbol;
}
console.log(inner());
console.log(typeof Symbol);
