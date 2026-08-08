// RFC 20260808 knife 3 (r330 registered defect #4,
// sm/object/15.2.3.4-02) — the typed-Arr receiver's keys / gOPN
// surfaces used to mint index strings off `arr.len` alone, so
// props-bag expandos never appeared and literal holes were counted
// as own indices. Both surfaces now box the cell and ride the full
// anyv_own_keys arr arm (exotic-aware index walk + expando tail).
var b = [1, 2];
b.p = 9;
console.log(Object.getOwnPropertyNames(b));
console.log(Object.keys(b));

// sparse literal: holes are not own properties
var c = [1, , , 7];
console.log(Object.getOwnPropertyNames(c));
console.log(Object.keys(c));

// the registered sm shape: unannotated var, reassignment, expando +
// non-enumerable defineProperty expando
var a, names;
a = [0, 1, 2];
names = Object.getOwnPropertyNames(a).sort();
console.log(names);
a = [1, , , 7];
a.p = 2;
Object.defineProperty(a, "q", { value: 42, enumerable: false });
console.log(Object.getOwnPropertyNames(a).sort());
console.log(Object.keys(a).sort());
a = [];
console.log(Object.getOwnPropertyNames(a));
