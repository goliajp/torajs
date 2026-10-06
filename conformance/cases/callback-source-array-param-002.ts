// a named function passed as an array callback, declaring the source
// array with the element type, on an `any[]` and on a typed receiver

interface P { v: number }
const an: any[] = [{ v: 5 }, { v: 6 }];
function named(p: any, i: number, arr: P[]) { console.log(arr.length, arr[i].v, p.v); }
an.forEach(named);
function named2(p: P, i: number, arr: P[]) { console.log(arr[1 - i].v + p.v); }
an.forEach(named2);
const ps: P[] = [{ v: 1 }, { v: 2 }];
ps.forEach(named);
ps.forEach(named2);
console.log(an.map(function m(p: any, i: number, arr: P[]) { return arr[i].v * 2; }).join(","));
