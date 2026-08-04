// Promise.any waits for elements that have not settled yet.
//
// It is the last of the four combinators to get its fan-in, and it
// reads the same machinery in a mirror: what short-circuits is a
// FULFILMENT, the count that has to reach zero is of rejections, and
// the indexed slots hold the errors list the AggregateError carries
// (§27.2.4.2.3's remainingElementsCount and its reject-element
// functions). It shares all's block rather than growing a second copy
// of the ownership protocol.
//
// Case A is the point: the element that fulfils is the one that
// settles LAST, so nothing can answer it without waiting. Case C is
// the mirror of that for rejections — the list is written by index, so
// its order is the input's and not the settling order.

function later(v: number, ticks: number): Promise<number> {
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

function failLater(v: string, ticks: number): Promise<string> {
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

// A — a rejection cannot win, so the outer waits for the fulfilment
// that lands several ticks later.
Promise.any([later(1, 6), later(2, 1)]).then((v) => {
  console.log("A", v);
});

// B — the earliest FULFILMENT wins even when a rejection settles
// first.
Promise.any([failLater("bad", 1), later(9, 4)]).then(
  (v: any) => {
    console.log("B", v);
  },
  (e: any) => {
    console.log("B-unreachable", e.name);
  }
);

// C — every element rejects: AggregateError, errors in INPUT order
// although index 1 rejected first.
Promise.any([failLater("e1", 4), failLater("e2", 1)]).then(
  (v: any) => {
    console.log("C-unreachable", v);
  },
  (e: any) => {
    console.log("C", e.name, JSON.stringify(e.errors), e instanceof AggregateError);
  }
);

// D — already-settled and pending elements mixed in one call.
Promise.any([Promise.reject("r0"), later(5, 2)]).then((v: any) => {
  console.log("D", v);
});

// E — any lane: a plain non-promise element is an already-fulfilled
// value, so it wins over anything still pending.
Promise.any([later(1, 3), 2]).then((v: any) => {
  console.log("E", v);
});

// F — any lane, every element rejects.
Promise.any([failLater("x", 3), Promise.reject(4)]).then(
  (v: any) => {
    console.log("F-unreachable", v);
  },
  (e: any) => {
    console.log("F", e.name, JSON.stringify(e.errors));
  }
);

// G — where the results land relative to a plain counter chain.
Promise.resolve(0)
  .then(() => {
    console.log("G t1");
  })
  .then(() => {
    console.log("G t2");
  })
  .then(() => {
    console.log("G t3");
  })
  .then(() => {
    console.log("G t4");
  })
  .then(() => {
    console.log("G t5");
  })
  .then(() => {
    console.log("G t6");
  })
  .then(() => {
    console.log("G t7");
  })
  .then(() => {
    console.log("G t8");
  });

console.log("sync-last");
