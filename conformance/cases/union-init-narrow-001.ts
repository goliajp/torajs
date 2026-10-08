// A union binding with an initializer reads as the initializer's
// member until something else is stored (TS reads the initializer as
// the first assignment); an assignment, a loop back edge and a closure
// bring the other members back.

let x: string | number = "hi";
console.log(x.toUpperCase(), x.length);

const y: string | number = 42;
console.log(y.toFixed(2));

let z: string | number = "a";
z = 5;
console.log(z.toFixed(1));

let w: string | number = "start";
for (let i = 0; i < 3; i++) {
  if (typeof w === "number") {
    console.log("num", w.toFixed(1));
  } else {
    console.log("str", w.toUpperCase());
  }
  w = i * 1.5;
}

let c: string | number = "c";
const show = (): string => (typeof c === "number" ? c.toFixed(2) : c.toUpperCase());
console.log(show());
c = 7;
console.log(show());

function first(xs: number[]): number | string {
  let r: number | string = "empty";
  if (xs.length > 0) r = xs[0];
  return r;
}
console.log(first([]), first([9]));
