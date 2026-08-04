// The any-lane half of §27.2.1.3.2 — a `.then` reached through an
// any-held receiver adopts a returned promise exactly as the typed
// kernels do. The two lanes disagreeing is the shape that let a
// handler's outcome depend on how its receiver happened to be typed.

const p: any = Promise.resolve(1);

p.then(() => Promise.resolve(2)).then((v: any) => console.log("A", v));

p.then(() => Promise.reject(new Error("b1"))).catch((e: any) =>
  console.log("B", e.message),
);

async function plus41(x: any) {
  return x + 41;
}
p.then((v: any) => plus41(v)).then((v: any) => console.log("C", v));

// a plain return still fulfills verbatim through the same lane
p.then((v: any) => v + 100).then((v: any) => console.log("D", v));

// and a throw still rejects through it
p.then(() => {
  throw new Error("e1");
}).catch((e: any) => console.log("E", e.message));
