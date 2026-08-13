// proposal-array-from-async §2.1.1 step 5.e calls the mapfn as
// `Call(mapfn, thisArg, «v, k»)`. With the thisArg omitted the answer
// is the no-receiver one, so a function expression in that slot reads
// it instead of refusing to compile.

async function main(): Promise<void> {
    const a = await Array.fromAsync([1, 2], function (v: number) {
        console.log("map", typeof this, v);
        return v * 10;
    });
    console.log("a", a[0], a[1]);

    // the source binding spelling
    const src = [3, 4];
    const b = await Array.fromAsync(src, function (v: number) {
        console.log("bound-src", typeof this, v);
        return v + 1;
    });
    console.log("b", b[0], b[1]);

    // no mapfn at all still works
    const c = await Array.fromAsync([5, 6]);
    console.log("c", c[0], c[1]);
}

main();
