// D1 — scalar concrete args through an Any receiver
const a: any[] = [1];
const b = a.concat(3);
console.log(b);
const c = a.concat("s");
console.log(c);
const d = a.concat(true, 2.5);
console.log(d);
// Any-typed scalar arg
const x: any = 7;
console.log(a.concat(x));
// Any+Any
const e: any[] = ["p", 2];
console.log(a.concat(e));
// Any + typed array arg
const t: number[] = [8, 9];
console.log(a.concat(t));
// 0-arg shallow copy
const f = e.concat();
console.log(f);
// chained multi-shape
console.log(a.concat(e, 5, t, "z"));
// receiver never mutates
console.log(a, a.length, e, e.length);
// owned-temp arg (fresh slice) + str temp
const g = a.concat(e.slice(0, 1), "li" + "t");
console.log(g);
