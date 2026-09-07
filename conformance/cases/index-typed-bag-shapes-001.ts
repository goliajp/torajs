// A keyed read or write on a shape whose runtime cell keeps an
// own-property bag boxes into the any lane and lands in that bag. The
// two keyed channels (read, write) ask one predicate, so a shape
// cannot be admitted to one and refused by the other.

const d = new Date(0);
console.log((d as any)[0]);
(d as any)[0] = 1;
console.log((d as any)[0]);
console.log(d.getTime());

const p = Promise.resolve(1);
console.log((p as any)[2]);
(p as any)[2] = 7;
console.log((p as any)[2]);

const mi = new Map([[1, 2]]).entries();
(mi as any)[0] = 6;
console.log((mi as any)[0]);
console.log(mi.next().value[0]);

const si = new Set([9]).values();
(si as any)[0] = 10;
console.log((si as any)[0]);
console.log(si.next().value);

// name keys on the same shapes keep working
(d as any).z = 11;
console.log((d as any).z);
