// An all-rejected Promise.any answers an AggregateError (§27.2.4.2).
//
// It used to forward the LAST rejection reason instead — the MVP
// posture, and one that reads as an ordinary defect from userland: a
// catch asks for `e.name` / `e.errors` and gets undefined off a bare
// string, and `e instanceof AggregateError` is false.
//
// The class is a TS-level one the compiler injects, so the runtime
// reaches it through the same native-error factory registry the thrown
// errors use; `Promise.any` implies the injection the way bigint
// division implies RangeError, since nothing in the call names it.
//
// Case B is the point of the errors list: §27.2.4.2.1's reject-element
// functions write `errors[index]`, so the order is the INPUT's — the
// list is pre-sized and written by index rather than pushed.
//
// Elements that are still pending are NOT covered here: this kernel
// cannot wait, and keeps forwarding until the fan-in lands.

// A — every element rejected, typed lane.
Promise.any([Promise.reject("r1"), Promise.reject("r2")]).then(
  (v) => {
    console.log("A-unreachable", v);
  },
  (e: any) => {
    console.log("A", e.name, JSON.stringify(e.errors), e instanceof AggregateError);
  }
);

// B — the list is indexed by element position, and carries no message
// of its own (`.message` falls through to the prototype's "").
Promise.any([Promise.reject("first"), Promise.reject("second")]).then(
  (v) => {
    console.log("B-unreachable", v);
  },
  (e: any) => {
    console.log("B", e.errors[0], e.errors[1], e.errors.length, "[" + e.message + "]");
  }
);

// The empty iterable lives in its own case file, where the tick it
// settles on is the subject — see promise-any-aggregate-002.
//
// D — any lane: a mixed literal infers Array<Any> and routes to the
// sibling kernel, which has to answer the same shape.
Promise.any([Promise.reject("r1"), Promise.reject(2)]).then(
  (v: any) => {
    console.log("D-unreachable", v);
  },
  (e: any) => {
    console.log("D", e.name, JSON.stringify(e.errors));
  }
);

// E — heap reasons survive into the list, which co-owns each one.
Promise.any([Promise.reject("s1"), Promise.reject({ k: 3 })]).then(
  (v: any) => {
    console.log("E-unreachable", v);
  },
  (e: any) => {
    console.log("E", e.name, JSON.stringify(e.errors));
  }
);

// F — a fulfilment still wins, and a plain non-promise element is an
// already-fulfilled value.
Promise.any([Promise.reject("r1"), 7]).then(
  (v: any) => {
    console.log("F", v);
  },
  (e: any) => {
    console.log("F-unreachable", e.name);
  }
);

console.log("sync-last");
