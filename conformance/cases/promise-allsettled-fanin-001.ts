// allSettled waits for elements that have not settled yet, and its
// records hold values rather than box bits.
//
// The placeholder rejection a pending element used to produce is an
// especially strange thing for the one combinator that per §27.2.4.3
// never rejects at all. The fan-in reuses Promise.all's block — same
// counter, same indexed slots — and differs in two places: a rejected
// element contributes a record instead of settling the result, and the
// slot holds that record rather than a raw value.
//
// Case C is the second half. An executor-minted element settles through
// the any lane, so its slot carries a NaN box while the record's field
// is typed T; the record read back {"value":-562949953421311}. That was
// invisible until the records became readable at all, and it is not
// specific to the fan-in — the synchronous path had it too, which is
// what case D pins.

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

function failLater(v: number, ticks: number): Promise<number> {
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

// A — pending elements, one of which rejects: nothing short-circuits,
// and the slots stay in array order rather than settle order.
Promise.allSettled([later(1, 6), failLater(9, 1), Promise.resolve(3)]).then((a: any) => {
  console.log("A", JSON.stringify(a));
});

// B — heap values through the fan-in. Declared Promise<string> on
// purpose: an unannotated `new Promise(...)` literal infers Array<Any>
// and routes to the any-lane sibling, whose fan-in is not written yet.
function strLater(v: string): Promise<string> {
  return new Promise((res) => {
    Promise.resolve(0).then(() => {
      res(v + "-tail");
    });
  });
}

Promise.allSettled([strLater("s1")]).then((b: any) => {
  console.log("B", JSON.stringify(b));
});

// C — the value is a value, not the bits of the box it arrived in.
Promise.allSettled([later(42, 2)]).then((c: any) => {
  console.log("C", c[0].status, c[0].value);
});

// D — the same, on the synchronous path (nothing here is pending).
Promise.allSettled([
  new Promise((res) => {
    res(7);
  }),
  Promise.resolve(8),
]).then((d: any) => {
  console.log("D", JSON.stringify(d));
});

// E — where the results land relative to a plain counter chain.
Promise.resolve(0)
  .then(() => {
    console.log("E t1");
  })
  .then(() => {
    console.log("E t2");
  })
  .then(() => {
    console.log("E t3");
  })
  .then(() => {
    console.log("E t4");
  })
  .then(() => {
    console.log("E t5");
  })
  .then(() => {
    console.log("E t6");
  })
  .then(() => {
    console.log("E t7");
  })
  .then(() => {
    console.log("E t8");
  });

console.log("sync-last");
