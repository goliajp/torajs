// An implicitly-generic function that returns an object literal over
// its parameter, read as a value: the relay's return is inferred from
// the forwarding call and used to come back spelled over the template's
// type variable (`__inlobj(v:__T1|s:string)`), which nothing binds for a
// closure-shaped declaration — the checker rejected the whole program.
// The relay's return floors to `any`, like every other un-typeable
// closure return (rotation 637).
function make(x) { return { v: x, s: "a" }; }
var z: any = make;
console.log(z(1).v, make(2).s, z("q").v + make("r").s, z === make);
function regular(n) { return { many: n, few: "f", one: "o" }; }
var r: any = regular;
console.log(r(3).many, regular(4).few);
