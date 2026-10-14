// A sloppy function with simple parameters has a mapped arguments
// object however it is called: `arguments[k]` and parameter k are one
// binding when the caller passed position k, and two when it did not.
eval("function M(a, b){ arguments[0] = 9; console.log('M', a, arguments[0], arguments.length); a = 5; console.log('M2', a, arguments[0]); arguments[1] = 7; console.log('M3', b, arguments[1]) }");
M(1, 2);
M.call(null, 1, 2);
var aliasM = M;
aliasM(1, 2);
new M(1, 2);
M(1);
M();
eval("function T(a, b){ var self = this; arguments[0] = 9; console.log('T', a, arguments[0], arguments.length, self === globalThis); a = 5; console.log('T2', a, arguments[0]); arguments[1] = 7; console.log('T3', b, arguments[1]) }");
T(1, 2);
T.call(null, 1, 2);
var aliasT = T;
aliasT(1, 2);
new T(1, 2);
T(1);
eval("function U(a){ arguments[0] += 10; console.log('U', a); arguments[0]++; console.log('U2', a, arguments[0]); a++; console.log('U3', arguments[0]) }");
U(1);
var aliasU = U;
aliasU(2);
aliasU();
function L(a){ var self = this; arguments[0] = 9; console.log('L', a, self === globalThis) }
var aliasL = L;
aliasL(1);
new aliasL(1);
aliasL();
function S(a){ "use strict"; arguments[0] = 9; console.log('S', a, arguments[0]) }
var aliasS = S;
aliasS(1);
