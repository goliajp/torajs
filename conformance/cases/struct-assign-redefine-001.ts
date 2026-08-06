// §20.1.2.1 step 4.c — Object.assign copies the source's ENUMERABLE
// own keys. With both sides statically typed the member list is
// unfolded at compile time, so the source stands behind the same
// redefined-member gate the values / entries unfolds use.

const src = { k: 1, l: 2 };
const dst = { k: 0, l: 0 };
Object.defineProperty(src as any, "k", { enumerable: false });
Object.assign(dst, src);
console.log(dst.k, dst.l, JSON.stringify(dst));
// The source still has the member; it is hidden, not gone.
console.log(src.k, "k" in src, JSON.stringify(src));

// Nothing hidden — the plain unfold, unchanged.
const s2 = { k: 7, l: 8 };
const d2 = { k: 0, l: 0 };
Object.assign(d2, s2);
console.log(d2.k, d2.l);

// Hiding every member copies nothing.
const s3 = { k: 5, l: 6 };
const d3 = { k: -1, l: -2 };
Object.defineProperty(s3 as any, "k", { enumerable: false });
Object.defineProperty(s3 as any, "l", { enumerable: false });
Object.assign(d3, s3);
console.log(d3.k, d3.l);

// Un-hiding puts it back in the copy set.
Object.defineProperty(s3 as any, "l", { enumerable: true });
Object.assign(d3, s3);
console.log(d3.k, d3.l);

// A string-valued member takes the same route.
const s4 = { a: "x", b: "y" };
const d4 = { a: "", b: "" };
Object.defineProperty(s4 as any, "a", { enumerable: false });
Object.assign(d4, s4);
console.log(JSON.stringify(d4));
