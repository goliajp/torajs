// §13.15.3 → ToString on a pointer-shaped slot the checker typed
// `T | undefined`. The `+` operator has a coercion of its own, and
// its nullish arm only ever tested NULL — which is how `null` is
// spelled in that slot, not how `undefined` is. The immortal cell
// rode the live-value arm into ToPrimitive, which asked a cell with
// no `toString` for one.

// --- the three pointer shapes, holding undefined, both operand sides
const o: { a: number } | undefined = undefined;
console.log("" + o);
console.log(o + "");
console.log("v=" + o + "!");

const xs: number[] | undefined = undefined;
console.log("" + xs);

class C {
  a = 1;
}
const c: C | undefined = undefined;
console.log("" + c);

// --- the same slots holding a live value keep their old arms
const o2: { a: number } | undefined = { a: 1 };
console.log("" + o2);
const xs2: number[] | undefined = [1, 2];
console.log("" + xs2);
const c2: C | undefined = new C();
console.log("" + c2);

// --- a class with its own toString still runs it
class D {
  toString(): string {
    return "D!";
  }
}
const d: D | undefined = new D();
console.log("" + d);
const dU: D | undefined = undefined;
console.log("" + dU);

// --- the null half of the same slot is unchanged
const n: { a: number } | null = null;
console.log("" + n);

// --- the string family, whose oddball the box already decoded
const s: string | undefined = undefined;
console.log("" + s);
const s2: string | undefined = "live";
console.log("" + s2);

// --- and the plain non-nullable spellings take the arms they took
console.log("" + { a: 1 });
console.log("" + [1, 2]);
console.log("" + 7);
console.log("" + true);
console.log("" + null);
console.log("" + undefined);
