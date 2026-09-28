export const other = 1;
const helper = () => (console.log("init r"), 5);
export const r = helper();
function g(r) { return r + 1; }
export function shown() { return g(r) + r; }
const arrow = (r) => r * 2;
const outer = (x) => ((r) => r + x)(r);
console.log("lib body", g(r), arrow(r), outer(100), shown());
export function f(n) { return n > 0 ? f(n - 1) + 1 : 0; }
function h(f) { return f(2); }
console.log("lib f", h(f), f(3));
