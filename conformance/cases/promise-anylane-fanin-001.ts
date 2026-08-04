// The any lane waits for pending elements too.
//
// all and allSettled learned to wait on the typed lane first. Left
// there, whether a pending element gets waited on would depend on
// whether the array literal happened to infer Array<Any> — a mixed
// literal like [somePromise, 2] does, and routes to the any-lane
// siblings, which still answered with the placeholder rejection. That
// is the same lane asymmetry the reaction handlers had: the observable
// behaviour turning on the receiver's static type rather than on what
// the program is doing.
//
// §27.2.4.1 treats a non-thenable element as an already-fulfilled
// value, so the plain 2 below contributes at once and never holds the
// result up.

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

// A — pending promise mixed with a plain value.
Promise.all([later(1, 3), 2]).then((a: any) => {
  console.log("A", a[0], a[1], a.length);
});

// B — a rejection still wins, and still carries its reason.
Promise.all([later(1, 4), failLater("bad", 1)]).then(
  (a: any) => {
    console.log("A-unreachable", a[0]);
  },
  (e: any) => {
    console.log("B rejected", e);
  }
);

// C — allSettled over the same mixed shape never rejects.
Promise.allSettled([later(7, 2), failLater("oops", 1), 8]).then((c: any) => {
  console.log("C", JSON.stringify(c));
});

// D — heap values survive the trip.
Promise.all([
  new Promise((res) => {
    Promise.resolve(0).then(() => {
      res("s");
    });
  }),
  "plain",
]).then((d: any) => {
  console.log("D", d[0], d[1]);
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
  });

console.log("sync-last");
