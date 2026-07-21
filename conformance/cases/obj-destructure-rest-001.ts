// Object destructuring rest (`{ p, ...rest } = obj`) — recorded as a
// parser gap 2026-06-17, verified working 2026-07-22; lock the shape.
let obj = { p: 1, q: 2, r: 3 };
let { p, ...rest } = obj;
console.log(p, JSON.stringify(rest));
let { q, ...rest2 } = { q: "x", s: [1, 2], t: false };
console.log(q, JSON.stringify(rest2));
