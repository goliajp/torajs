// A function declaration that writes a binding outside its body,
// passed by name over an escaped array and an any[]
let last = 0;
function rec(v: number) {
  last = v;
}
let total = 0;
function add(v: number): number {
  total += v;
  return total;
}
let seen = "";
function note(v: number): void {
  seen = seen + v + ";";
}

const d = [1, 2, 3];
const u: any = d;
u[1] = 5;
d.forEach(rec);
console.log(last, typeof last);
console.log(d.map(add).join(","), total);
d.forEach(note);
console.log(seen);

const a: any[] = [6, 7];
a.forEach(rec);
console.log(last, typeof last);
console.log(a.map(add).join(","), total);
