// a branch that cannot complete narrows what follows it, whether it
// returns, calls a function declared to return never, exits the
// process, or returns from inside a try
type R = { status: "ok"; value: number } | { status: "err"; msg: string };

function fail(m: string): never {
  throw new Error(m);
}

function viaNever(r: R): number {
  if (r.status === "err") {
    fail(r.msg);
  }
  return r.value;
}

function viaExit(r: R): number {
  if (r.status === "err") {
    process.exit(3);
  }
  return r.value + 1;
}

function viaTry(r: R): number {
  if (r.status === "err") {
    try {
      return -1;
    } finally {
      console.log("finally", r.msg);
    }
  }
  return r.value + 2;
}

type A = { k: "a"; x: number };
type B = { k: "b"; y: string };

function viaSwitch(u: A | B): number {
  switch (u.k) {
    case "a":
      try {
        return u.x;
      } finally {
      }
    case "b":
      return u.y.length;
  }
}

console.log(viaNever({ status: "ok", value: 4 }));
try {
  viaNever({ status: "err", msg: "boom" });
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(viaExit({ status: "ok", value: 5 }));
console.log(viaTry({ status: "ok", value: 6 }), viaTry({ status: "err", msg: "m" }));
console.log(viaSwitch({ k: "a", x: 7 }), viaSwitch({ k: "b", y: "abcd" }));
