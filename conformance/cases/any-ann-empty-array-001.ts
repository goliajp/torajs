// rotation 73 L3b — `const e: any = []` promotes the literal to
// Arr<Any> boxed into the any slot (chunk-809 any-ann family);
// previously the checker rejected it while bun accepts.
const empty: any = [];
console.log(empty.length);
empty.push(1);
empty.push("two");
console.log(empty.length, empty[0], empty[1]);
console.log(Array.isArray(empty));
let m: any = [];
m[0] = 5;
console.log(m.length, m[0]);
