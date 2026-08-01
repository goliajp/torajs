// rotation 275 刀 2 — Array.fromAsync / Promise.try / JSON.rawJSON /
// JSON.isRawJSON join the ns-static intern table: name / length /
// typeof reflection plus the detached-call faces (fromAsync falls to
// ArrayCreate on its undefined |this|; Promise.try raises the step-1
// TypeError; the JSON pair run their real same-crate kernels).

// ---- reflection: length / name / typeof ----
console.log(Array.fromAsync.length, Array.fromAsync.name, typeof Array.fromAsync);
console.log(Promise.try.length, Promise.try.name, typeof Promise.try);
console.log(JSON.rawJSON.length, JSON.rawJSON.name, typeof JSON.rawJSON);
console.log(JSON.isRawJSON.length, JSON.isRawJSON.name, typeof JSON.isRawJSON);

// ---- hasOwnProperty over the namespaces ----
console.log(Array.hasOwnProperty("fromAsync"));
console.log(Promise.hasOwnProperty("try"));
console.log(JSON.hasOwnProperty("rawJSON"), JSON.hasOwnProperty("isRawJSON"));

// ---- detached JSON pair: the real kernels ----
const raw = JSON.rawJSON;
const isRaw = JSON.isRawJSON;
console.log(JSON.stringify(raw(1)));
console.log(JSON.stringify(raw('"x"')));
console.log(isRaw(raw(37)));
console.log(isRaw({ rawJSON: "37" }));
try {
  raw("{}");
  console.log("no throw");
} catch (e) {
  console.log((e as Error).name);
}

// ---- detached Promise.try: undefined |this| TypeError ----
const t = Promise.try;
try {
  t(() => 1);
  console.log("no throw");
} catch (e) {
  console.log((e as Error).name);
}

// ---- detached Array.fromAsync: ArrayCreate fallback ----
const fa = Array.fromAsync;
const a = await fa([1, 2, 3]);
console.log(a.join(","));
const b = await fa([1, 2], (x: any, i: any) => x * 10 + i);
console.log(b.join(","));

// ---- §10.4.2.2 ArrayCreate step 1: length > 2^32-1 rejects ----
try {
  await fa({ length: 4294967296 });
  console.log("no throw");
} catch (e) {
  console.log((e as Error).name);
}
