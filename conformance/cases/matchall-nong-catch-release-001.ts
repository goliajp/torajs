// matchAll on a non-global regex throws a catchable TypeError; the
// mint-and-throw kernel's answered cell must not strand (churn probe
// verified); semantics: name + subsequent g-flag call still works
const s: any = "abcabc";
const bad: any = /b/;
let caught = "";
try {
  s.matchAll(bad);
} catch (e: any) {
  caught = e.name;
}
console.log(caught);

// optchain variant takes the same throw path
let caught2 = 0;
try {
  s?.matchAll(bad);
} catch {
  caught2 = 1;
}
console.log(caught2);

// good g-flag matchAll still answers matches afterwards
const good: any = /b/g;
let n = 0;
let firstHit = "";
let secondIdx = -1;
for (const m of s.matchAll(good)) {
  if (n === 0) {
    firstHit = m[0];
  }
  if (n === 1) {
    secondIdx = m.index;
  }
  n += 1;
}
console.log(n);
console.log(firstHit);
console.log(secondIdx);
