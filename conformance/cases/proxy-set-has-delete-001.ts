// §10.5.9 [[Set]] / §10.5.7 [[HasProperty]] / §10.5.10 [[Delete]].
const log: string[] = [];
const t: any = { a: 1 };

const p: any = new Proxy(t, {
  set(target: any, key: any, value: any, recv: any) {
    log.push("set:" + String(key) + "=" + String(value));
    target[key] = value;
    return true;
  },
  has(target: any, key: any) {
    log.push("has:" + String(key));
    return key === "magic" || key in target;
  },
  deleteProperty(target: any, key: any) {
    log.push("del:" + String(key));
    delete target[key];
    return true;
  },
});

p.b = 2;
p[0] = "zero";
console.log(t.b, t[0]);
console.log("a" in p, "magic" in p, "nope" in p);
console.log(delete p.b, t.b);
console.log(log.join(" | "));

// A trap that refuses.
const strictly: any = new Proxy({}, { set() { return false; } });
try { strictly.x = 1; } catch (e: any) { console.log("set refused:", e instanceof TypeError); }

const undeletable: any = new Proxy({ k: 1 }, { deleteProperty() { return false; } });
try { console.log(delete undeletable.k); } catch (e: any) { console.log("del refused:", e instanceof TypeError); }

// A trap answering a falsish non-boolean refuses just the same.
const zeroish: any = new Proxy({}, { has() { return 0 as any; } });
console.log("anything" in zeroish);

// No traps at all — everything forwards to the target.
const q: any = new Proxy({ z: 9 }, {});
q.w = 4;
console.log(q.z, q.w, "z" in q, "nope" in q, delete q.z, "z" in q);

// A trap that throws propagates.
const boom: any = new Proxy({}, { set() { throw new RangeError("nope"); } });
try { boom.x = 1; } catch (e: any) { console.log(e instanceof RangeError, e.message); }
