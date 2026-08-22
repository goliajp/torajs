// §28.2.2.1 Proxy.revocable + §10.5.x revoked-proxy TypeErrors.
const target: any = { a: 1 };
const r: any = Proxy.revocable(target, { get(t: any, k: any) { return "T" + String(k); } });
const p: any = r.proxy;
console.log(typeof r, typeof p, typeof r.revoke);
console.log(p.a);
console.log("a" in p);

console.log(r.revoke());
// Every internal method now throws.
try { p.a; } catch (e: any) { console.log("get:", e instanceof TypeError); }
try { p.a = 2; } catch (e: any) { console.log("set:", e instanceof TypeError); }
try { "a" in p; } catch (e: any) { console.log("has:", e instanceof TypeError); }
try { delete p.a; } catch (e: any) { console.log("del:", e instanceof TypeError); }

// Revoking twice is a no-op.
console.log(r.revoke());

// The target is untouched.
console.log(target.a);

// The revoke function's own reflection face.
console.log(r.revoke.length, JSON.stringify(r.revoke.name));

// A revocable proxy with no traps forwards until revoked.
const r2: any = Proxy.revocable({ z: 5 }, {});
console.log(r2.proxy.z);
r2.revoke();
try { r2.proxy.z; } catch (e: any) { console.log("post-revoke:", e instanceof TypeError); }

// Bad arguments throw the same §10.5.14 TypeError.
try { Proxy.revocable(1 as any, {}); } catch (e: any) { console.log("bad target:", e instanceof TypeError); }
