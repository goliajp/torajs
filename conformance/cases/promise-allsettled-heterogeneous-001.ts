// `Promise.allSettled` over elements whose settled values have no
// single form — the mirror of what `Promise.all` had.
//
// There the slots of the RESULT ARRAY were read through one element's
// repr. Here it is the value slot of each `{status, value}` RECORD:
// the checker types a heterogeneous call `{status, value: Any}`, so
// the slot must hold a NaN box, and a raw number's bits sat there
// instead — reading back as null.
//
// A string survived, which is what made a representation bug look like
// a per-type one, and why the first note about this recorded the wrong
// discriminator (element count rather than heterogeneity).

// the shape: one number, one string
Promise.allSettled([Promise.resolve(2), Promise.resolve("s")]).then((v: any) => {
  console.log("num-str", v[0].status, v[0].value, v[1].status, v[1].value);
});

// a rejected element makes the pair heterogeneous too — this is the
// commonest way to meet it, since the reason is rarely the value's type
Promise.allSettled([Promise.resolve(2), Promise.reject(new Error("x"))]).then((v: any) => {
  console.log("num-rej", v[0].value, v[1].status, v[1].reason.message);
});

// other value forms on the other side
Promise.allSettled([Promise.resolve(2), Promise.resolve(true)]).then((v: any) => {
  console.log("num-bool", v[0].value, v[1].value);
});
Promise.allSettled([Promise.resolve(2), Promise.resolve(null)]).then((v: any) => {
  console.log("num-null", v[0].value, v[1].value);
});
Promise.allSettled([Promise.resolve(2), Promise.resolve([9])]).then((v: any) => {
  console.log("num-arr", v[0].value, v[1].value[0]);
});

// the fan-in lane, which a pending element takes
Promise.allSettled([Promise.resolve(2), Promise.resolve().then(() => "s")]).then((v: any) => {
  console.log("fanin-het", v[0].value, v[1].value);
});
Promise.allSettled([
  Promise.resolve().then(() => 2),
  Promise.resolve().then(() => "s"),
]).then((v: any) => {
  console.log("fanin-het2", v[0].value, v[1].value);
});

// an any-shape INPUT reaches the same record shape for its own reason
Promise.allSettled([Promise.resolve(2), "plain"]).then((v: any) => {
  console.log("anylane", v[0].value, v[1].value);
});

// homogeneous inputs must not move: their records keep a raw slot
Promise.allSettled([Promise.resolve(2)]).then((v: any) => {
  console.log("one-num", v[0].status, v[0].value);
});
Promise.allSettled([Promise.resolve(2), Promise.resolve(3)]).then((v: any) => {
  console.log("two-num", v[0].value, v[1].value);
});
Promise.allSettled([Promise.resolve("a"), Promise.resolve("b")]).then((v: any) => {
  console.log("two-str", v[0].value, v[1].value);
});
Promise.allSettled([Promise.resolve(true), Promise.resolve(false)]).then((v: any) => {
  console.log("two-bool", v[0].value, v[1].value);
});
Promise.allSettled([]).then((v: any) => {
  console.log("empty", v.length);
});
