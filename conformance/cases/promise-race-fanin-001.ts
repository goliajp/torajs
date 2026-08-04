// §27.2.4.5 Promise.race over elements that are still PENDING at call
// time. The combinator used to reject with a placeholder because it
// could not wait; it now attaches one reaction per element and the
// first settlement wins.

function pend(v: number, ticks: number): Promise<number> {
  return new Promise((res) => {
    let p = Promise.resolve(0);
    for (let i = 0; i < ticks; i++) {
      p = p.then(() => 0);
    }
    p.then(() => {
      res(v);
    });
  });
}

// the LATER element settles first — array order must not decide it
Promise.race([pend(1, 5), pend(2, 1)]).then((v) => console.log("A", v));

// array order does decide among elements that settle on the same tick
Promise.race([pend(7, 1), pend(8, 1)]).then((v) => console.log("B", v));

// a rejection wins the race just as a fulfilment does
function rejLater(): Promise<number> {
  return new Promise((res, rej) => {
    Promise.resolve(0).then(() => {
      rej(new Error("c1"));
    });
  });
}
Promise.race([rejLater()]).then(
  (v) => console.log("C-unexpected", v),
  (e: any) => console.log("C", e.message),
);

// an already-settled element still wins over a pending one
Promise.race([Promise.resolve(9), pend(10, 3)]).then((v) => console.log("D", v));

// an empty iterable is forever pending — the handler never runs
Promise.race([]).then(() => console.log("E-unexpected"));
console.log("E-sync");
