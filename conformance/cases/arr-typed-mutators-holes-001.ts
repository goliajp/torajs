// The in-place mutators move a number[]'s holes with its elements
// (a write past the end leaves them): reverse, shift, unshift, fill,
// copyWithin, splice (and its removed array), pop.
function mk(): number[] {
  let a = [1.5, 2.5];
  a[4] = 3.5;
  return a;
}
console.log(mk().reverse());
{ const a = mk(); a.reverse(); console.log(0 in a, 1 in a, 2 in a, 3 in a, 4 in a); }
{ const a = mk(); console.log(a.shift(), a, a.length); }
{ const q = mk(); while (q.length) q.shift(); console.log(q, q.length); }
{ const a = mk(); a.unshift(9); console.log(a); }
console.log(mk().fill(7, 2, 3));
console.log(mk().fill(7));
console.log(mk().copyWithin(0, 2));
console.log(mk().copyWithin(1, 2, 4));
{ const a = mk(); const r = a.splice(1, 2); console.log(a, r, r.length); }
{ const a = mk(); a.splice(0, 0, 7); console.log(a); }
{ const a = mk(); a.pop(); a.pop(); console.log(a, a.length); a.push(8); a.push(9); console.log(a); }
{ const a = mk(); console.log(a.pop(), a.pop(), a.pop(), a, a.length); }
const d = [1, 2, 3];
d.reverse();
d.shift();
d.unshift(0);
console.log(d, d.pop(), d);
