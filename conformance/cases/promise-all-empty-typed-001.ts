// Promise.all over an empty `Promise<number>[]`: the result is an empty
// array a `then` handler can read, fulfilled one tick after the jobs
// queued ahead of it.
const order: string[] = [];
const none: Promise<number>[] = [];
Promise.resolve(0).then(() => order.push("tick"));
Promise.all(none).then((v) => order.push("empty " + v.length));
Promise.resolve(0).then(() => 0).then(() => console.log(order.join(", ")));
const strs: Promise<string>[] = [];
Promise.all(strs).then((v) => console.log("strs", v.length, JSON.stringify(v)));
