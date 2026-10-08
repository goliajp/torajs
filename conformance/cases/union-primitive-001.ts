// `A | B` over primitive types (TS §3.4) as a parameter, a return
// type, a binding and an array element. The value is whichever member
// was stored; typeof and printing answer that member.

function show(v: string | number): string {
  return "v=" + v + " (" + typeof v + ")";
}
console.log(show(1), show("one"));

function pick(b: boolean): string | number {
  return b ? "yes" : 42;
}
console.log(pick(true), pick(false), typeof pick(false));

let x: string | number | boolean = 1;
console.log(x, typeof x);
x = "two";
console.log(x, typeof x);
x = false;
console.log(x, typeof x);

const arr: (string | number)[] = [1, "two", 3];
arr.push("four");
for (const e of arr) console.log(typeof e, e);
console.log(arr.length, arr.join("|"));

const gen: Array<string | number> = ["a", 2];
console.log(gen[0], gen[1], gen.length);

function sum(xs: (number | string)[]): number {
  let total = 0;
  for (const e of xs) total += Number(e);
  return total;
}
console.log(sum([1, "2", 3.5]));

// the member order of a union does not matter
function rev(v: number | string): string | number {
  return v;
}
console.log(rev(show(7)), rev(8));

// an optional union parameter left out is undefined
function opt(p?: number | string): string {
  return "p=" + p + " " + typeof p;
}
console.log(opt(), opt(1), opt("s"));

// a default value for a union parameter
function dflt(v: string | number = 5): string | number {
  return v;
}
console.log(dflt(), dflt("k"));
