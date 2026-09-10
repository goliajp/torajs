// An un-annotated top-level let / const bound to a CLASS VALUE is a data global a named
// function body can read (K.3b any-promotion): the class object is an any-lane cell.
class C { v() { return 45; } static tag = "C!"; }
const alias = C;
let mutableAlias = C;
const viaAlias = alias;
function f(): any { return alias; }
function g(): any { return { default: alias }; }
function h(): any { return mutableAlias.tag + viaAlias.tag; }
console.log(new (f())().v(), g().default === C, f() === C, alias.name, h());
console.log(new alias().v(), alias === C);
