// §27.2.2.1 PromiseReactionJob steps 8-9 — a handler that completes
// abruptly rejects the derived promise. Covers every typed kernel:
// .then(cb), .catch(cb), the native 2-arg .then(ok, err) (both legs),
// and .finally, in bare-fn and capturing-closure shapes.

// .then handler throws -> downstream .catch sees it
Promise.resolve(1)
  .then(() => {
    throw new Error("t1");
  })
  .catch((e: any) => console.log("A", e.message));

// .catch handler throws -> its own result rejects
Promise.reject(new Error("t2"))
  .catch(() => {
    throw new Error("t3");
  })
  .catch((e: any) => console.log("B", e.message));

// 2-arg .then, fulfilled leg throws
Promise.resolve(1).then(
  () => {
    throw new Error("t4");
  },
  () => 0,
).catch((e: any) => console.log("C", e.message));

// 2-arg .then, rejection leg throws -> replaces the reason
Promise.reject(new Error("t5")).then(
  () => 0,
  () => {
    throw new Error("t6");
  },
).catch((e: any) => console.log("D", e.message));

// .finally throwing wins over the settlement it was forwarding
Promise.resolve(7)
  .finally(() => {
    throw new Error("t7");
  })
  .catch((e: any) => console.log("E", e.message));

// closure shape — the handler captures, so it rides the env variant
const tag = "t8";
Promise.resolve(1)
  .then(() => {
    throw new Error(tag);
  })
  .catch((e: any) => console.log("F", e.message));

// a handler that returns normally still fulfills
Promise.resolve(2)
  .then((v) => v + 1)
  .catch(() => -1)
  .then((v) => console.log("G", v));
