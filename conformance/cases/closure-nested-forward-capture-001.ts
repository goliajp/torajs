// Forward references from closures minted DEEPER in a let-init
// (rotation 260): the hoist passes only looked at inits that ARE a
// closure, so an object-literal method / array element capturing a
// binding declared later in the same list answered "references
// unknown identifier" / "closure capture not in scope" — even
// though ES hoists the `let` and the body only runs after the
// declaration. Covers toplevel + fn body, the chained shape
// (hoisted init referencing an earlier same-list binding), and
// keeps mutual recursion / the plain-binding hoist on their
// pre-existing lanes.
let o = {
  next() {
    return other + 1;
  },
};
let other = 41;
console.log(o.next());
function h() {
  let p = { next() { return inner * 2; } };
  let inner = 21;
  return p.next();
}
console.log(h());
let holder = {
  read() {
    return box.v;
  },
};
let seed = { v: 7 };
let box = seed;
console.log(holder.read());
const isEven = (n: number): boolean => (n === 0 ? true : isOdd(n - 1));
const isOdd = (n: number): boolean => (n === 0 ? false : isEven(n - 1));
console.log(isEven(10), isOdd(7));
const g2 = () => o2.v;
const o2 = { v: 3 };
console.log(g2());
let arr = [
  function () {
    return late * 2;
  },
];
let late = 21;
console.log(arr[0]());
