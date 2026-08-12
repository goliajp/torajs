// Rotation 373 (L3b 373-02) — §21.4.2.1 step 5: every supplied
// component of the multi-argument Date constructor runs ToNumber in
// argument order. The desugar wraps non-Number-literal components in
// the `Number(x)` coercion call, so string digits, booleans, user
// valueOf hooks and evaluation order all ride the existing m1.h.8
// machinery.

// 1. string components coerce (the t262 regular-subclassing shape)
const d1 = new Date(1859, "10" as any, 24, 11);
console.log("str-month", d1.getFullYear(), d1.getMonth(), d1.getDate());

// 2. mixed literal kinds: bool → 1, null → +0
const d2 = new Date(1970, true as any, 1);
console.log("bool-month", d2.getMonth());
const d3 = new Date(1970, null as any, 5);
console.log("null-month", d3.getMonth(), d3.getDate());

// 3. runtime string variables coerce too
let m = "11";
const d4 = new Date(2000, m as any, 31);
console.log("var-month", d4.getMonth(), d4.getDate());

// 4. a user valueOf hook runs, in argument order
const order: number[] = [];
const mk = (n: number) => ({
  valueOf() {
    order.push(n);
    return n;
  },
});
const d5 = new Date(mk(2001) as any, mk(5) as any, mk(9) as any);
console.log("hook", d5.getFullYear(), d5.getMonth(), d5.getDate(), JSON.stringify(order));

// 5. a non-numeric string component invalidates the whole date
const d6 = new Date(2000, "x" as any, 1);
console.log("bad-comp", Number.isNaN(d6.getTime()));

// 6. two-arg form pads day=1 and still coerces
const d7 = new Date(1999, "0" as any);
console.log("two-arg", d7.getMonth(), d7.getDate());
