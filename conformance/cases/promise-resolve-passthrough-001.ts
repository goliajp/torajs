// §27.2.4.7 step 2 — Promise.resolve on a promise answers the SAME
// object; reject always mints.
const p1 = Promise.resolve(1);
const p2 = Promise.resolve(p1);
console.log(p1 === p2);
const p3 = Promise.resolve(p2);
console.log(p3 === p1);
p2.then((v) => {
  console.log(v);
});
const r1 = Promise.reject(p1);
const r2 = Promise.resolve(r1);
console.log(r1 === r2);
r2.catch((e) => {
  console.log(e === p1);
});
