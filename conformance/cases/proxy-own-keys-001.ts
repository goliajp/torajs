// §10.5.11 [[OwnPropertyKeys]] + §10.5.5 [[GetOwnProperty]].
const t: any = { a: 1, b: 2 };

const p: any = new Proxy(t, {
  ownKeys(target: any) { return ["x", "y", "a"]; },
  getOwnPropertyDescriptor(target: any, key: any) {
    if (key === "y") return undefined;
    return { value: "V" + String(key), enumerable: true, configurable: true };
  },
});
console.log(Object.keys(p).join(","));
console.log(Object.getOwnPropertyNames(p).join(","));
const d: any = Object.getOwnPropertyDescriptor(p, "a");
console.log(d.value, d.enumerable, d.writable, d.configurable);
console.log(Object.getOwnPropertyDescriptor(p, "y"));

// A partial descriptor is completed per §6.2.6.6.
const q: any = new Proxy({}, {
  getOwnPropertyDescriptor() { return { value: 7, configurable: true }; },
  ownKeys() { return ["k"]; },
});
const dq: any = Object.getOwnPropertyDescriptor(q, "k");
console.log(dq.value, dq.writable, dq.enumerable, dq.configurable);
// Non-enumerable, so Object.keys drops it but gOPN keeps it.
console.log(Object.keys(q).length, Object.getOwnPropertyNames(q).join(","));

// No traps — both forward to the target.
const r: any = new Proxy(t, {});
console.log(Object.keys(r).join(","));
console.log(Object.getOwnPropertyDescriptor(r, "a").value);

// The ownKeys trap must answer an object.
const bad: any = new Proxy({}, { ownKeys() { return 1 as any; } });
try { Object.keys(bad); } catch (e: any) { console.log("ownKeys:", e instanceof TypeError); }

// The gOPD trap must answer an object or undefined.
const bad2: any = new Proxy({}, { getOwnPropertyDescriptor() { return 1 as any; } });
try { Object.getOwnPropertyDescriptor(bad2, "z"); } catch (e: any) { console.log("gOPD:", e instanceof TypeError); }

// Object.values / entries ride the same pair.
console.log(JSON.stringify(Object.values(p)));
console.log(JSON.stringify(Object.entries(p)));
