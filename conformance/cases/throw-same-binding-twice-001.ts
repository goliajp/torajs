// The same non-Copy binding thrown more than once. The checker used to
// refuse the second throw ("cannot transfer — value was already aliased
// or moved earlier"), standing in for a retain contract the lowering was
// thought to be missing. It was not missing: a throw retains into the
// throw slot and leaves the binding its own drop, so each catch gets a
// live object and the source survives the whole program.
//
// Three shapes, because the refusal sat on the binding rather than on
// any one of them: two throws in sequence, a throw on the path a return
// did not take, and a throw inside a loop (where the count of transfers
// is not even static).

var o = { a: 1 };
try {
    throw o;
} catch (e: any) {
    console.log("first", e.a);
}
try {
    throw o;
} catch (e: any) {
    console.log("second", e.a);
}
console.log("source survives", o.a);

function pick(c: boolean): any {
    var s = { v: 7 };
    if (c) {
        return s;
    }
    throw s;
}
console.log("returned", pick(true).v);
try {
    pick(false);
} catch (e: any) {
    console.log("thrown", e.v);
}

var g = { n: 0 };
for (var i = 0; i < 3; i = i + 1) {
    try {
        throw g;
    } catch (e: any) {
        e.n = e.n + 1;
    }
}
console.log("loop mutated through the catch", g.n);
