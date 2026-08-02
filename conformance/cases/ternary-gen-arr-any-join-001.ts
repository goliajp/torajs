// rotation 284 — the dstr default guard mints a ternary joining an
// `Array(Any)` source element against a generator instance
// (`x === undefined ? g() : x`). The join answers Any (both sides
// box) and the empty-pattern destructure walks the any iterator
// protocol on whichever branch ran. g's body never starts (a
// generator call only mints the object), so callCount stays 0 in
// both the default-unused and default-used shapes.
var callCount = 0;
function* g() { callCount += 1; }
const [[,] = g()] = [[]];
console.log(callCount);
const [[] = g()] = [];
console.log(callCount);
