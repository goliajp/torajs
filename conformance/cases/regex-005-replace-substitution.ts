// Phase 1c.2: $& / $N / $$ substitution in s.replace / s.replaceAll.

const r1 = "hello world".replace(/(\w+) (\w+)/, "$2 $1");
console.log(r1);

const r2 = "abc".replace(/b/, "[$&]");
console.log(r2);

const r3 = "x12y34z".replaceAll(/\d+/g, "<$&>");
console.log(r3);

const r4 = "$ price".replace(/\$/, "USD$$");
console.log(r4);

// $01 → group 1 (two-digit normalize)
const r5 = "abc".replace(/(b)/, "[$01-$1]");
console.log(r5);

// String-only path still works
const r6 = "hello".replace("ll", "rr");
console.log(r6);

// $N for N > n_captures — keeps "$" literal but consumes digit (JS spec quirk)
const r7 = "abc".replace(/b/, "[$2]");
console.log(r7);

// Combined captures
const r8 = "John Smith".replace(/(\w+)\s(\w+)/, "$2, $1");
console.log(r8);
