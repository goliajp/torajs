// tr infers an unannotated callback parameter from the declaration
// it can see, before anything knows about narrowing. So in
// `s = "hi"; Promise.resolve(s).then((v) => …)` the handler is
// inferred `(v: string | null) => …` while the receiver — which does
// see the narrow — is `Promise<string>`, and the two faces of one
// line disagreed hard enough to refuse the program.
//
// Contravariance settles it: a handler declaring the wider parameter
// still receives the narrower value. tr adds one bound, because the
// parameter is a slot — `T | null` and `T` are the same slot only
// when T is pointer-shaped. A scalar's nullable rides a NaN-boxed
// lane instead, and handing it a raw f64 would be the same lie the
// narrow itself is not allowed to tell.

let s: string | null;
console.log("pre:", s);
s = "hi";
Promise.resolve(s).then((v) => {
  console.log("narrowed:", v);
});

// The scalar spelling reaches the same place by the boxed lane.
let n: number | null;
console.log("pre:", n);
n = 3;
Promise.resolve(n).then((v) => {
  console.log("scalar:", v);
});

let b: boolean | null;
console.log("pre:", b);
b = true;
Promise.resolve(b).then((v) => {
  console.log("bool:", v);
});

// A guard narrows the same way an assignment does.
const g: string | null = "gg";
if (g !== null) {
  Promise.resolve(g).then((v) => {
    console.log("guard:", v);
  });
}

// A second hop reads the first handler's return, not the source.
let c: string | null;
console.log("pre:", c);
c = "cc";
Promise.resolve(c)
  .then((v) => {
    console.log("hop1:", v);
  })
  .then(() => {
    console.log("hop2");
  });

// An un-narrowed nullable still rides its own lane, unchanged.
const u: string | null = null;
Promise.resolve(u).then((v) => {
  console.log("union:", v);
});

// An explicitly annotated handler was never affected.
let d: string | null;
console.log("pre:", d);
d = "dd";
Promise.resolve(d).then((v: string) => {
  console.log("annotated:", v);
});
