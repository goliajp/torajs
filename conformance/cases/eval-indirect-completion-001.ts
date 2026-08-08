// A closed multi-statement indirect eval source (binds nothing, no
// free variables) completes with its trailing expression anywhere —
// scope cannot tell the IIFE from the global evaluation (§19.2.1.1).
console.log((0, eval)("1; 2;"));
console.log((0, eval)("'use strict'; 7; 8;"));
var t = 0;
function inner(): number {
  return (0, eval)("5; 6;");
}
console.log(inner());
console.log((0, eval)("if (true) { 1; } 9;"));
