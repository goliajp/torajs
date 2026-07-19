// RFC 20260719-ns-static-value-reify B2 — console stdout-family
// statics read as VALUES (log / info / debug; error / warn stay the
// recorded loud boundary until an any-print stderr kernel exists).
const f = console.log;
f("hi");
f(42);
console.log(console.log);
console.log(typeof f);
console.log(f === console.log);
console.log(f.name);
console.log(f.length);
console.log(f.toString());

const i: any = console.info;
i("info-line");
console.log(typeof i);
console.log(i.name);

const d = console.debug;
d("debug-line");
console.log(d.name);
