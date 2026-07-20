// RFC 20260720-promise-any-cb knife 3 — two-arg
// `.then(onOk, onErr)` with `(v: any) => R` in either (or both)
// slots, mixed with the classic `(v: T) => T` shape; the fulfilled
// leg's return drives the chained result type.
Promise.resolve(11).then(
  function (v: any) {
    console.log("ok-any", v);
  },
  function (e: number) {
    console.log("err-typed", e);
    return e;
  }
);
Promise.reject(22).then(
  function (v: number) {
    console.log("ok-typed", v);
    return v;
  },
  function (e: any) {
    console.log("err-any", e);
  }
);
Promise.reject(33).then(
  function (v: any) {
    console.log("ok2", v);
  },
  function (e: any) {
    console.log("err2", e);
  }
);
Promise.resolve(44)
  .then(
    function (v: any) {
      return 55;
    },
    function (e: any) {
      console.log("e3", e);
    }
  )
  .then(function (w: any) {
    console.log("chain", w);
  });
