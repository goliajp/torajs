// [[Get]] reaches every spelling of a read, not just `p.name`.
const arr: any = [10, 20, 30];
const p: any = new Proxy(arr, {
  get(t: any, k: any) {
    if (k === "length") return 99;
    return "K" + String(k);
  },
});
console.log(p[0], p[1], p.length);
const k: any = "0";
console.log(p[k]);

// Trap-less proxy over an array forwards index and length reads.
const q: any = new Proxy(arr, {});
console.log(q[0], q[2], q.length);

// A method call is Call(GetV(p, "m"), p, args).
const obj: any = { v: 5, m(this: any) { return this.v; } };
const seen: any[] = [];
const r: any = new Proxy(obj, {
  get(t: any, key: any, recv: any) {
    seen.push(String(key));
    return t[key];
  },
});
console.log(r.m());
console.log(seen.join(","));

// `this` inside the invoked method is the proxy, so its own read
// goes back through the trap.
console.log(seen.length);

// A non-callable answer for a call site is a TypeError.
const bad: any = new Proxy({}, { get() { return 1; } });
try { bad.nope(); } catch (e: any) { console.log(e instanceof TypeError); }

// Trap-less proxy over an array still answers array methods.
console.log(q.join("-"));
console.log(q.indexOf(20));
