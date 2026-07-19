// combinator results crossing into the any lane (RFC
// 20260720-anylane-promise-methods knife 3): runtime-side repr stamp
// + result-array elem-kind mark from the source repr
const c1: any = Promise.all([Promise.resolve(1), Promise.resolve(2)]);
c1.then((v: any) => { console.log("all", v[0] + v[1]); });
const c2: any = Promise.race([Promise.resolve(7), Promise.resolve(8)]);
c2.then((v: any) => { console.log("race", v); });
const c3: any = Promise.any([Promise.resolve(9), Promise.resolve(10)]);
c3.then((v: any) => { console.log("any", v); });
const c4: any = Promise.all([Promise.resolve("a"), Promise.resolve("b")]);
c4.then((v: any) => { console.log("all-str", v[0] + v[1]); });
console.log("sync");
