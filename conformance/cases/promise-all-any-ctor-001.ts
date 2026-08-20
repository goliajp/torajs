// §27.2.4.1 over an arbitrary constructor |this|: PerformPromiseAll
// mints one §27.2.4.1.3 function per element over a shared values
// list and a remaining-elements counter, and hands the finished array
// to the capability's own resolve function.
let resolved: any = null;
let rejected: any = null;

function C(executor) {
  executor(
    function (v) {
      resolved = v;
    },
    function (e) {
      rejected = e;
    },
  );
}
C.resolve = function (v) {
  return v;
};

// Each element's `then` is invoked with that element's own resolve
// function; calling it synchronously fills its slot in order.
const mk = function (v) {
  return {
    then: function (onFulfilled, onRejected) {
      onFulfilled(v);
    },
  };
};

Promise.all.call(C, [mk("a"), mk("b"), mk("c")]);
console.log(Array.isArray(resolved), resolved.length, resolved.join(","));
console.log("rejected", rejected);

// §27.2.4.1.3 steps 1-3: the element function is single-shot, so a
// second call after Promise.all returned changes nothing.
let seen: any = null;
let later: any = null;
function C2(executor) {
  executor(
    function (v) {
      seen = v;
    },
    function () {},
  );
}
C2.resolve = function (v) {
  return v;
};
const p1 = {
  then: function (onFulfilled, onRejected) {
    later = onFulfilled;
    onFulfilled("first");
  },
};
Promise.all.call(C2, [p1]);
console.log(seen.join(","));
later("second");
console.log(seen.join(","));

// An empty iterable resolves with an empty array right away.
let empty: any = null;
function C3(executor) {
  executor(
    function (v) {
      empty = v;
    },
    function () {},
  );
}
C3.resolve = function (v) {
  return v;
};
Promise.all.call(C3, []);
console.log(Array.isArray(empty), empty.length);

// The reject side is the capability's own function, shared by every
// element.
let why: any = null;
let sameReject = 0;
function C4(executor) {
  executor(function () {}, function (e) {
    why = e;
  });
}
C4.resolve = function (v) {
  return v;
};
let capReject: any = null;
const q1 = {
  then: function (onFulfilled, onRejected) {
    capReject = onRejected;
  },
};
const q2 = {
  then: function (onFulfilled, onRejected) {
    if (onRejected === capReject) {
      sameReject += 1;
    }
    onRejected("boom");
  },
};
Promise.all.call(C4, [q1, q2]);
console.log("same reject fn", sameReject, "why", why);
