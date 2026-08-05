// `Promise.resolve(p)` where `p` is an `any` holding a promise answers
// the INNER cell (§27.2.4.7 step 2). The inner cell's stamp says how
// its value is stored; the call site's static type, erased to
// `Promise<any>`, says nothing. A typed handler declared its own lane,
// and the two disagreed — the raw value went over unconverted.
function mkn(): any {
  return Promise.resolve(7);
}
function mkf(): any {
  return Promise.resolve(2.5);
}
function mks(): any {
  return Promise.resolve("hi");
}
function mkb(): any {
  return Promise.resolve(true);
}

// a typed handler over a pass-through cell: the stamp is what decides
Promise.resolve(mkn()).then((x: number) => {
  console.log("n", x);
  return 0;
});
Promise.resolve(mkf()).then((x: number) => {
  console.log("f", x);
  return 0;
});
Promise.resolve(mks()).then((x: string) => {
  console.log("s", x);
  return 0;
});
Promise.resolve(mkb()).then((x: boolean) => {
  console.log("b", x);
  return 0;
});

// the two lanes that already worked, kept as neighbours: an `any`
// handler boxes per the stamp, and a directly-minted cell agrees with
// its handler by construction
Promise.resolve(mkn()).then((x: any) => {
  console.log("a", x);
  return 0;
});
const q: any = Promise.resolve(mkn());
q.then((x: any) => {
  console.log("via-any", x);
  return 0;
});
Promise.resolve(7).then((x: number) => {
  console.log("plain", x);
  return 0;
});
Promise.resolve("z").then((x: string) => {
  console.log("pstr", x);
  return 0;
});
