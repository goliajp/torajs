// The prefix type assertion `<T>expr` is `expr as T` spelled before the
// operand: a cast for the checker, identity at run time.

const c: any = 5;
const n = <number>c;
console.log(n + 1, (<any>c).toFixed(1));

const s: any = "abc";
console.log((<string>s).length, <string>s + "!");

const xs: any = [3, 1, 2];
const arr = <Array<number>>xs;
console.log(arr.length, arr[0]);

const k = <const>["a", "b"];
console.log(k.length, k[1]);

interface P { x: number }
const raw: any = { x: 7 };
const p = <P>raw;
console.log(p.x);

console.log(-<number>c, !<boolean>c, typeof <any>s);
