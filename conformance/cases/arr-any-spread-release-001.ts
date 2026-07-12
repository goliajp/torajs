// any-product spread source release — behavior must stay byte-equal
// (the fix is an rc-ledger release; this fixture guards no double-free
// and unchanged values across both spread lanes)
const s: any = "a1b2c3";

// lane A: [...anyCallProduct] — owned Call product as spread source
const parts = [...s.split("b")];
console.log(parts.length);
console.log(parts[0]);
console.log(parts[1]);

// repeated occurrence in a loop — double-free would crash here
for (let i = 0; i < 3; i++) {
  const p = [...s.split("1")];
  console.log(p.length, p[0], p[1]);
}

// lane A borrow shape regression: spread of a plain binding (Ident
// self-gates — no release, source must stay alive after)
const src: any = [10, 20, 30];
const copy1 = [...src];
const copy2 = [...src];
console.log(copy1.length, copy2.length, src.length);
console.log(copy1[2], copy2[0]);

// lane B: any source spread inside an Array<Any> literal with heads
const mixed: any = [0, ...s.split("c"), 99];
console.log(mixed.length);
console.log(mixed[0], mixed[1], mixed[2], mixed[3]);

// nested owned product: matchAll answers an array of match arrays
const t: any = "xabyab";
const ms = [...t.matchAll(/ab/g)];
console.log(ms.length);
console.log(ms[0][0], ms[1][0]);

// chained: spread product consumed further
const upper = [...s.split("2")].length;
console.log(upper);
