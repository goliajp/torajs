// Promise.all over elements whose settled values have no single form.
//
// SSA's Type::Promise is inner-erased, so an Array<Promise<T>> input
// looks homogeneous whatever the T's are: every slot is a plain promise
// pointer. The result array's slots are what differ, and the kernel used
// to read all of them through the FIRST element's form. That is silent,
// not loud — a Str pointer surfaces as a number, `true` as 1.
//
// The checker knew all along: it types this call Promise<Array(Any)>,
// and hands the element form down as the combinator's target_repr. Both
// lanes now take that fork — the synchronous one below, and the fan-in
// one that runs when an element is still pending.

// --- synchronous lane (every element already settled) ---
Promise.all([Promise.resolve(2), Promise.resolve("s")]).then((v: any) => {
  console.log("num-str", v[0], v[1]);
});
Promise.all([Promise.resolve("a"), Promise.resolve(2)]).then((v: any) => {
  console.log("str-num", v[0], v[1]);
});
Promise.all([Promise.resolve(2), Promise.resolve(true)]).then((v: any) => {
  console.log("num-bool", v[0], v[1]);
});
Promise.all([Promise.resolve(2), Promise.resolve(null)]).then((v: any) => {
  console.log("num-null", v[0], v[1]);
});
Promise.all([Promise.resolve(2), Promise.resolve(undefined)]).then((v: any) => {
  console.log("num-undef", v[0], v[1]);
});
Promise.all([Promise.resolve(2), Promise.resolve([9])]).then((v: any) => {
  console.log("num-arr", v[0], v[1][0]);
});

// --- fan-in lane (a pending element makes the jobs run) ---
Promise.all([Promise.resolve(2), Promise.resolve().then(() => "s")]).then((v: any) => {
  console.log("fanin-het", v[0], v[1]);
});
Promise.all([
  Promise.resolve().then(() => 2),
  Promise.resolve().then(() => "s"),
]).then((v: any) => {
  console.log("fanin-het2", v[0], v[1]);
});

// --- homogeneous inputs must NOT move to the boxed lane ---
// These keep raw slots: the call site names one form, so the result
// array carries an elem-kind chain exactly as before. int/float mix is
// deliberately here — both are f64-faced, so it is not heterogeneous.
Promise.all([Promise.resolve(2), Promise.resolve(3)]).then((v: any) => {
  console.log("homo-num", v[0], v[1]);
});
Promise.all([Promise.resolve("a"), Promise.resolve("b")]).then((v: any) => {
  console.log("homo-str", v[0], v[1]);
});
Promise.all([Promise.resolve(2), Promise.resolve(1.5)]).then((v: any) => {
  console.log("homo-intfloat", v[0], v[1]);
});
Promise.all([Promise.resolve(2), Promise.resolve(3)]).then((v: number[]) => {
  console.log("typed-static", v[0] + v[1]);
});
Promise.all([]).then((v: any) => {
  console.log("empty", v.length);
});

// --- the any-shape INPUT lane, which already answered correctly ---
// It reaches the same result shape for the other reason; keeping it here
// means a change that collapses the two reasons back together shows up.
Promise.all([Promise.resolve(2), "s"]).then((v: any) => {
  console.log("anylane-het", v[0], v[1]);
});
Promise.all([Promise.resolve(2), 5]).then((v: any) => {
  console.log("anylane-plain", v[0], v[1]);
});

// --- a rejected element still short-circuits ---
Promise.all([Promise.resolve(1), Promise.reject(new Error("boom"))]).then(
  (v: any) => {
    console.log("unreachable", v);
  },
  (e: any) => {
    console.log("all-reject", e.message);
  },
);
