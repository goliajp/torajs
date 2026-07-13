// RFC 20260713-loose-eq-substrate fix-up — a Date's ToPrimitive
// with the default hint walks toString first (§21.4.4.45), so
// `date == string` compares the toString form, not epoch millis.

const d = new Date(0);
console.log(d == d.toString());
console.log(d.toString() == d);
console.log(d == 0);
console.log(d != d.toString());

let ad: any = d;
console.log(ad == d.toString());
console.log(ad == 0);

// identity stays identity
const d2 = new Date(0);
console.log(d == d);
console.log(d == d2);
