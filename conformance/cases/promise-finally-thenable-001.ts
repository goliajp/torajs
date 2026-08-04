// .finally waits for a thenable its handler returns (§27.2.5.3).
//
// The handler's return used to have nowhere to go: the callback type
// was `fn()`, so the value never left the register, and a handler
// declared to return anything at all was a COMPILE reject —
// `.finally(() => 99)` did not build, and neither did the shape the
// wait exists for, `.finally(() => cleanupAsync())`.
//
// What the result settles with is still the SOURCE's settlement, not
// the handler's value. The one thing that displaces it is a returned
// promise that REJECTS: the `.then(() => value)` §27.2.5.3 builds has
// no onRejected, so that rejection comes through on either leg —
// cases D and E.

function delayed(v: number, ticks: number): Promise<number> {
  return new Promise((res) => {
    let p = Promise.resolve(0);
    for (let i = 0; i < ticks; i++) {
      p = p.then((x: number) => x);
    }
    p.then(() => {
      res(v);
    });
  });
}

function rejLater(v: string, ticks: number): Promise<string> {
  return new Promise((res, rej) => {
    let p = Promise.resolve(0);
    for (let i = 0; i < ticks; i++) {
      p = p.then((x: number) => x);
    }
    p.then(() => {
      rej(v);
    });
  });
}

// A — the handler returns a pending promise, so the source's value
// arrives only after it settles. Case F pins how much later.
Promise.resolve(1)
  .finally(() => delayed(0, 4))
  .then((v) => {
    console.log("A", v);
  });

// B — same on the rejected leg.
Promise.reject("boom")
  .finally(() => delayed(0, 4))
  .then(
    (v: any) => {
      console.log("B-unreachable", v);
    },
    (e: any) => {
      console.log("B", e);
    }
  );

// C — a non-thenable return is discarded, and a heap one is released
// rather than stranded.
Promise.resolve(3)
  .finally(() => "discarded")
  .then((v) => {
    console.log("C", v);
  });

// D — the returned promise rejects: its reason wins over a fulfilled
// source.
Promise.resolve(4)
  .finally(() => rejLater("from-finally", 2))
  .then(
    (v: any) => {
      console.log("D-unreachable", v);
    },
    (e: any) => {
      console.log("D", e);
    }
  );

// E — and over a rejected one.
Promise.reject("src")
  .finally(() => rejLater("from-finally", 2))
  .then(
    (v: any) => {
      console.log("E-unreachable", v);
    },
    (e: any) => {
      console.log("E", e);
    }
  );

// F — a closure handler (it captures `tag`), still argument-free, and
// where everything lands against a plain counter chain.
const tag = "cleanup";
Promise.resolve(6)
  .finally(() => {
    console.log("F ran", tag);
    return delayed(0, 1);
  })
  .then((v) => {
    console.log("F", v);
  });

// G — the void handler that always worked keeps working.
Promise.resolve(7)
  .finally(() => {
    console.log("G ran");
  })
  .then((v) => {
    console.log("G", v);
  });

Promise.resolve(0)
  .then(() => {
    console.log("t1");
  })
  .then(() => {
    console.log("t2");
  })
  .then(() => {
    console.log("t3");
  })
  .then(() => {
    console.log("t4");
  })
  .then(() => {
    console.log("t5");
  })
  .then(() => {
    console.log("t6");
  })
  .then(() => {
    console.log("t7");
  })
  .then(() => {
    console.log("t8");
  });

console.log("sync-last");
