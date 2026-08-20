// §27.2.4.5 names |this| as the species constructor C, and asks only
// that C be a constructor — NewPromiseCapability(C) reaches it through
// Construct, and PerformPromiseRace reaches everything else through
// Call and Invoke. A plain user function is a constructor.
function resolveFunction(v) {
  console.log("resolved", v);
}
function rejectFunction(e) {
  console.log("rejected", e);
}

function C(executor) {
  console.log("ctor ran");
  executor(resolveFunction, rejectFunction);
}
C.resolve = function (v) {
  console.log("C.resolve", v);
  return v;
};

let calls = 0;
const p1 = {
  then: function (onFulfilled, onRejected) {
    calls += 1;
    // Every element gets the SAME capability functions, verbatim.
    console.log("p1.then", onFulfilled === resolveFunction, onRejected === rejectFunction);
  },
};
const p2 = {
  then: function (onFulfilled, onRejected) {
    calls += 1;
    console.log("p2.then", onFulfilled === resolveFunction, onRejected === rejectFunction);
  },
};

const out = Promise.race.call(C, [p1, p2]);
console.log("calls", calls);
console.log("out is ctor instance", out instanceof C);

// C.resolve is consulted once per element (a plain value rides it
// too — the algorithm never inspects the element before the call).
let seen = [];
function C2(executor) {
  executor(function () {}, function () {});
}
C2.resolve = function (v) {
  seen.push(v);
  return { then: function () {} };
};
Promise.race.call(C2, [1, 2]);
console.log("C2.resolve saw", seen.join(","));

// A non-constructor |this| still raises the step-1 TypeError.
try {
  Promise.race.call({}, []);
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
