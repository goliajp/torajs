// A call to an `async function` is a syntactically certain promise
// (§27.7.5.1 always builds the result with the intrinsic %Promise%),
// so a function expression in its handler slot reads the
// no-receiver answer §27.2.5.4 step 9 passes.

async function one(): Promise<number> {
    return 1;
}

async function two(): Promise<number> {
    return 2;
}

// directly on the call
one().then(function (v: number) {
    console.log("direct", typeof this, v);
});

// through a const binding
const p = one();
p.then(function (v: number) {
    console.log("bound", typeof this, v);
});

// chained over a handler method — the chain stays certain
one()
    .then(function (v: number) {
        console.log("chain-1", typeof this, v);
        return v + 1;
    })
    .then(function (v: number) {
        console.log("chain-2", typeof this, v);
    });

// catch / finally slots on an async call
two()
    .catch(function (e: any) {
        console.log("catch", typeof this, e);
    })
    .finally(function () {
        console.log("finally", typeof this);
    });

// a fixed-point chain written across statements
const a = two();
const b = a.then(function (v: number) {
    console.log("fix-a", typeof this, v);
    return v;
});
b.then(function (v: number) {
    console.log("fix-b", typeof this, v);
});

// a plain (non-async) function returning a promise is NOT admitted by
// the async-name rule, but its Promise-static body still is
function madeByStatic(): Promise<number> {
    return Promise.resolve(9);
}
const q = madeByStatic();
console.log("plain-fn-call-not-async", typeof q);
