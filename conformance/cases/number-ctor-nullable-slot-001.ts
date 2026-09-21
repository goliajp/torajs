// §7.1.4 takes every value: ToNumber(undefined) is NaN. A slot the
// checker typed `T | undefined` was refused at type-check
// ("Number(Nullable(Struct([...]))) coercion not yet supported"),
// while the `String` arm beside it had admitted the same union an
// earlier rotation ago. Asking for NaN instead of a crash is what
// the call is for.

// --- the three pointer shapes, holding undefined
const o: { a: number } | undefined = undefined;
console.log(Number(o));

const xs: number[] | undefined = undefined;
console.log(Number(xs));

class C {
  a = 1;
}
const c: C | undefined = undefined;
console.log(Number(c));

// --- the same slots holding a live value run §7.1.4 step 8
const o2: { a: number } | undefined = { a: 1 };
console.log(Number(o2));

// (a nullable slot cannot hold an EMPTY array literal yet — the
// annotation `__nullable(number[])` is not one the empty-literal
// rule reads; registered, a different family)
const one: number[] | undefined = [7];
console.log(Number(one));
const two: number[] | undefined = [1, 2];
console.log(Number(two));

// a live receiver with its own valueOf still runs it
class V {
  valueOf(): number {
    return 42;
  }
}
const v: V | undefined = new V();
console.log(Number(v));
const vU: V | undefined = undefined;
console.log(Number(vU));

// --- the null half of the same slot is +0, not NaN
const n: { a: number } | null = null;
console.log(Number(n));

// --- the scalar and string families were already right
const num: number | undefined = undefined;
console.log(Number(num));
const s: string | undefined = undefined;
console.log(Number(s));
const s2: string | undefined = "12";
console.log(Number(s2));

// --- the plain non-nullable spellings keep their arms
console.log(Number({ a: 1 }));
console.log(Number([3]));
console.log(Number("5"));
console.log(Number(true));
console.log(Number(null));
console.log(Number(undefined));
