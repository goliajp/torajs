// ES 10.2.11 gives a declaration one function object, so every route
// to it must answer that one. A `this`-reading declaration stored into
// a property slot keeps its receiver by way of a receiver-first shell
// (`__fwdrecv_<name>`), and that shell is minted once per NAME — but
// only the PLAIN forwarder answered a canonical cell, so each store
// site evaluated its own object and two stores of one function were
// not equal to each other.
//
// The two shells are still distinct from each other (`o.m === t` is
// the cross-shell question, RFC 20260905 residue 1, open); this file
// pins the within-shell half, and the receiver each store carries.

function t(v: any): any {
  const th = this as any;
  return String(v) + (th === undefined ? "u" : "h:" + String(th.z));
}

const o: any = {};
const p: any = {};
o.m = t;
p.m = t;

// one function, one object — across two store sites
console.log(o.m === p.m);

// and the receiver is still the holder at each of them, which is the
// whole reason the receiver-first shell exists
o.z = 7;
p.z = 9;
console.log(o.m(1), p.m(1));

// a third store through a different spelling of the same slot
const q: any = {};
q["m"] = t;
q.z = 3;
console.log(q.m === o.m, q.m(2));

// the plain-call route is untouched: a bare call binds undefined
console.log(t(4));

// a receiver-claimed callback position still runs with the thisArg,
// and using the name at two of them does not mint two objects either
const r: any = { z: 5 };
const a: any[] = [1];
console.log(a.map(t, r).join(","), a.map(t, r).join(","));
