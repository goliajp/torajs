// The no-receiver handler slot reached through a name instead of
// written in the argument position. Every use of the binding has to
// be an admitted slot, since the binding materializes one closure and
// its `this` answer is the same everywhere it is used.

const onOk = function (v: number) {
    console.log("then", typeof this, v);
};
Promise.resolve(1).then(onOk);

const onOk2 = function (v: number) {
    console.log("chain", typeof this, v);
};
Promise.resolve(2).then(onOk2);

async function three(): Promise<number> {
    return 3;
}
const onAsync = function (v: number) {
    console.log("async-then", typeof this, v);
};
three().then(onAsync);

// already correct on HEAD without this pass: the array callbacks
// reach the callee through a plain user-fn argument position
const each = function (v: number) {
    console.log("forEach", typeof this, v);
};
[7, 8].forEach(each);
