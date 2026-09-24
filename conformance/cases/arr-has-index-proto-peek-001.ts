// `i in arr` on arrays with and without holes, before and after a digit
// key lands on Array.prototype — the prototype step only finds keys on a
// prototype somebody already fetched.
const a = [1, 2, 3];
console.log(1 in a, 5 in a, -1 in a);
const b: number[] = [1];
b[3] = 4;
console.log(0 in b, 1 in b, 2 in b, 3 in b, 4 in b);
const c: any[] = [1, , 3];
console.log(0 in c, 1 in c, 2 in c);
const g: number[] = [7, 8];
g.length = 4;
console.log(1 in g, 2 in g, 3 in g);
(Array.prototype as any)[1] = "p";
console.log(1 in b, 1 in c, 2 in b, 2 in g);
delete (Array.prototype as any)[1];
console.log(1 in b, 1 in c);
