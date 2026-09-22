// §13.5.1.2 step 5 — `delete o[k]` runs ToPropertyKey(k): a BigInt key
// names "12", a boolean "true", a number its §7.1.17 string. Each
// coerced key is released after the delete (the loops would double-free
// or leak otherwise).
const o: any = { 12: "a", true: "t", 5.5: "f", ab: 1, k: 1 };
const b: bigint = 12n;
console.log(delete o[b], delete o[true], JSON.stringify(o));
const f: number = 5.5;
const s = "xab";
console.log(delete o[f], delete o[s.slice(1)], JSON.stringify(o));
console.log(delete o[13n], delete o[false], JSON.stringify(o));
let c = 0;
for (let i = 0; i < 2000; i++) {
  const x: any = { 5: i, true: 1, 7: 2, 1.5: 3 };
  const n: number = 7;
  const g: number = 1.5;
  if (delete x[5n] && delete x[true] && delete x[n] && delete x[g]) c++;
  if (Object.keys(x).length === 0) c++;
}
console.log(c);
