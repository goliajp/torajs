// Promise.all waits for elements that have not settled yet.
//
// A pending element used to reject the result with a placeholder — the
// MVP's way of saying it could not wait — which turned the commonest
// shape in the family, `Promise.all([asyncCall(), asyncCall()])`, into
// an uncaught rejection. race got its wait first because the first
// settlement wins and there is nothing to collect; all also needs a
// count of outstanding elements and a slot per element to write into
// by index (§27.2.4.1.3's remainingElementsCount and its indexed
// resolve-element functions).
//
// Case A is the point of the whole thing: the element that settles
// LAST is the one at index 0, and the result must still read 1, 2.
// Case E pins the tick the result lands on against a counter chain —
// a fan-in that settles a microtask early or late shows up there.

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

function strLater(v: string): Promise<string> {
  return new Promise((res) => {
    Promise.resolve(0).then(() => {
      res(v + "-tail");
    });
  });
}

// A — slot order follows the array, not the settle order.
Promise.all([later(1, 6), later(2, 1)]).then((a) => {
  console.log("A", a[0], a[1]);
});

// B — the first rejection settles the result, and a later element
// settling afterwards does not disturb it.
Promise.all([later(1, 5), failLater(9, 1), later(3, 2)]).then(
  (a) => {
    console.log("A-unreachable", a[0]);
  },
  (e) => {
    console.log("B rejected", e);
  }
);

// C — already-settled and pending elements mixed in one call.
Promise.all([Promise.resolve(5), later(6, 2)]).then((c) => {
  console.log("C", c[0], c[1]);
});

// D — heap elements: the result array keeps its own stake on each,
// so they outlive the promises they were read out of.
Promise.all([strLater("x"), strLater("y")]).then((d) => {
  console.log("D", d[0], d[1], d.length);
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
