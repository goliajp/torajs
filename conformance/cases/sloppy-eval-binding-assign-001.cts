// §19.2.1: `eval` is an ordinary writable global function property. In sloppy code a
// program may assign to it or update it (test262 assignmenttargettype/*-eval and
// postfix-decrement/eval-nostrict); the name then reads the stored value.
var before = typeof eval;
eval = 1;
console.log(before, eval, typeof eval);
eval--;
console.log(eval);
eval += 10;
console.log(eval);
