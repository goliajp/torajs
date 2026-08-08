// RFC 20260808 knife 4 — ArraySpeciesCreate steps 5-10, the
// @@species half. Two fixes land together: the typed-Arr receiver's
// `.constructor` READ consults the own expando before the builtin
// virtual face (§10.1.8.1 — pre-fix `a.constructor = {}` wrote a bag
// entry no read ever saw, and the species write that followed landed
// on the builtin cell), and the species-family gate runs a callable
// @@species constructor so its abrupt completion is observable.
var Ctor = function () {
  throw new Error("poisoned-species");
};

function go(arr: any): any {
  return arr.slice(0, 1);
}

// anon-object-literal constructor + species write through the chain
var a = [1, 2, 3];
a.constructor = {};
a.constructor[Symbol.species] = Ctor;
try {
  go(a);
  console.log("no throw");
} catch (e) {
  console.log("caught:", e.message);
}

// explicit any binding spelling
var b = [4, 5];
var c: any = {};
b.constructor = c;
c[Symbol.species] = Ctor;
try {
  go(b);
  console.log("no throw");
} catch (e) {
  console.log("caught:", e.message);
}

// map with callback rides the same gate
var d = [6];
d.constructor = {};
d.constructor[Symbol.species] = Ctor;
var dd: any = d;
try {
  dd.map(function (x: any) {
    return x;
  });
  console.log("no throw");
} catch (e) {
  console.log("caught:", e.message);
}

// undefined species defaults (step 7.b) — no throw, plain product
var e2 = [7, 8];
e2.constructor = {};
var r: any = go(e2);
console.log(r);

// constructor identity face keeps the interned builtin
console.log([].constructor === Array);
var xs = [1];
console.log(xs.constructor === Array);
