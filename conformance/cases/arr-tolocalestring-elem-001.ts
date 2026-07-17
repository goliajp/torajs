// §23.1.3.32 — arr.toLocaleString() invokes each non-nullish
// element's OWN toLocaleString (observable hook, once per
// occurrence); undefined/null render empty; comma join. Pre-fix
// the Arr arm delegated to plain join and never dispatched the
// element method.

let n = 0;
const obj: any = {
  toLocaleString() { n++; return "X"; },
};
const arr: any = [undefined, obj, null, obj, obj];
console.log(arr.toLocaleString()); // ,X,,X,X
console.log(n); // 3

// numeric arrays keep the locale-format path
const nums: any = [1000, 2.5];
console.log(nums.toLocaleString()); // 1,000,2.5

// plain values stringify
const mix: any = [1, "a", true];
console.log(mix.toLocaleString()); // 1,a,true
console.log("done");
