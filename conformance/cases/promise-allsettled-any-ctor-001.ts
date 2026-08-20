// §27.2.4.3 / §27.2.4.6 over an arbitrary constructor |this|.
// allSettled mints a resolve/reject PAIR per element that share one
// [[AlreadyCalled]] record; any counts rejections down to an
// AggregateError on the capability's reject function.
let settled: any = null;
function CS(executor) {
  executor(
    function (v) {
      settled = v;
    },
    function () {},
  );
}
CS.resolve = function (v) {
  return v;
};

const ok = function (v) {
  return { then: function (onF, onR) { onF(v); } };
};
const bad = function (e) {
  return { then: function (onF, onR) { onR(e); } };
};

Promise.allSettled.call(CS, [ok("a"), bad("boom"), ok("c")]);
console.log(settled.length);
console.log(settled[0].status, settled[0].value);
console.log(settled[1].status, settled[1].reason);
console.log(settled[2].status, settled[2].value);
console.log("value on rejected", settled[1].value, "reason on fulfilled", settled[0].reason);

// The pair shares one [[AlreadyCalled]]: an element that resolves and
// then rejects records only the first answer.
let both: any = null;
function CS2(executor) {
  executor(
    function (v) {
      both = v;
    },
    function () {},
  );
}
CS2.resolve = function (v) {
  return v;
};
const twice = {
  then: function (onF, onR) {
    onF("first");
    onR("second");
  },
};
Promise.allSettled.call(CS2, [twice]);
console.log(both[0].status, both[0].value, both[0].reason);

// any: every element rejects, so the capability's REJECT gets an
// AggregateError carrying the reasons in iteration order.
let agg: any = null;
function CA(executor) {
  executor(function () {}, function (e) {
    agg = e;
  });
}
CA.resolve = function (v) {
  return v;
};
Promise.any.call(CA, [bad("x"), bad("y")]);
console.log(agg instanceof AggregateError, agg.name);
console.log(agg.errors.length, agg.errors.join(","));

// A single fulfilled element resolves through the capability instead.
let won: any = null;
let lost: any = null;
function CA2(executor) {
  executor(
    function (v) {
      won = v;
    },
    function (e) {
      lost = e;
    },
  );
}
CA2.resolve = function (v) {
  return v;
};
Promise.any.call(CA2, [bad("no"), ok("yes")]);
console.log(won, lost);

// An empty iterable rejects immediately with an empty AggregateError.
let none: any = null;
function CA3(executor) {
  executor(function () {}, function (e) {
    none = e;
  });
}
CA3.resolve = function (v) {
  return v;
};
Promise.any.call(CA3, []);
console.log(none instanceof AggregateError, none.errors.length);
