// RFC 20260727-dstr-decl-shape 刀 A — declaration-position
// destructuring through the recursive PatShape machine: nested
// patterns (array-in-array, obj-in-array, array-in-obj, obj-in-obj),
// defaults on simple and nested slots, elisions, rest, renames, and
// the combinations the flat walks refused outright.
//
// Recorded boundary (NOT covered here): a defaulted field read from
// a literal empty struct (`let { m = 5 } = {}`) is a pre-existing
// checker reject (no member on Struct([])) — L3b entry, this fixture
// keeps every source shape field-complete.

// nested array with default, empty source
let [[a, b] = [4, 5]] = [];
console.log(a, b);

// nested array, populated source overrides the default
let [[c, d] = [4, 5]] = [[1, 2]];
console.log(c, d);

// obj-in-obj
let { p: { q } } = { p: { q: 9 } };
console.log(q);

// obj-in-obj two levels with sibling bind
let { u: { v, w }, s } = { u: { v: 1, w: 2 }, s: 3 };
console.log(v, w, s);

// mixed: obj element and array element inside one array pattern
let [{ m }, [n]] = [{ m: 1 }, [2]];
console.log(m, n);

// array-in-obj
let { pair: [x1, y1] } = { pair: [7, 8] };
console.log(x1, y1);

// defaults + elision + rest in one pattern
let [e0 = 10, , e2, ...tail] = [undefined, 99, 3, 4, 5];
console.log(e0, e2, tail.length, tail[0], tail[1]);

// rename with default (value present / absent-by-undefined)
let { k1: r1 = 11, k2: r2 = 22 } = { k1: 1, k2: undefined };
console.log(r1, r2);

// object rest after nested sibling
let { g: { h }, ...others } = { g: { h: 1 }, i: 2, j: 3 };
console.log(h, others.i, others.j);

// nested-with-default where the loaded value wins
let { p2: { q2 = 5 } = { q2: 6 } } = { p2: { q2: 7 } };
console.log(q2);

// deep nesting: array in obj in array
let [{ inner: [z1, z2] }] = [{ inner: [30, 40] }];
console.log(z1, z2);

// var-form pattern (fn-scope leak stays a recorded divergence; the
// binding face itself must work)
var [va, vb] = [1, 2];
console.log(va, vb);
