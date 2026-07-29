// §10.4.3 — a String wrapper's inherent own face (every canonical
// index in range, plus `length`) is {configurable: false}, so a
// module-strict `delete` throws. Pre-fix the delete fell straight to
// the expando probe, found nothing, and answered true while the
// property stayed right where it was.
const w: any = new String("abcd");
const k = "1";
try { delete w[k]; console.log("deleted", w[k]); } catch (e) { console.log("threw", (e as any).constructor.name, w[k]); }
try { delete w["length"]; console.log("len deleted"); } catch (e) { console.log("len threw", (e as any).constructor.name, w["length"]); }

// an out-of-range index owns nothing — that delete is a spec success
const oob = "9";
console.log(delete w[oob]);

// a genuine expando still deletes
w.extra = 1;
console.log(delete w["extra"], w.extra);

// the own face survived all of it
console.log(w[k], w["length"], w[0], w[3]);
