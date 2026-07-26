// `push(a, b, c)` is split into sequential single-argument calls so
// the lowering never sees a variadic one. Only a receiver that was
// already a name got split — a field receiver was left alone, reached
// the lowering as a multi-argument push and stopped it:
//
//     const o: { xs: number[] } = { xs: [] };
//     o.xs.push(1, 2);
//     // not yet supported: unsupported member call shape: push
//
// The single-argument form was fine, which is what said the split was
// the only thing missing. Reusing the original receiver for each split
// call would evaluate `o.xs` once per argument, so it is read once
// into a name first — the hoisted form the note here used to tell
// people to write by hand.

const o: { xs: number[] } = { xs: [] };
o.xs.push(1, 2);
console.log("field-push", o.xs.length, o.xs[1]);

const o2: { xs: number[] } = { xs: [] };
o2.xs.push(1, 2, 3, 4);
console.log("field-push-four", o2.xs.length, o2.xs[3]);

const o3: { i: { xs: number[] } } = { i: { xs: [] } };
o3.i.xs.push(1, 2, 3);
console.log("nested-field-push", o3.i.xs.length, o3.i.xs[2]);

const o4: { xs: number[] } = { xs: [9] };
o4.xs.unshift(1, 2, 3);
console.log("field-unshift-order", o4.xs[0], o4.xs[2], o4.xs[3]);

// push answers the final length, from a field receiver too.
const o5: { xs: number[] } = { xs: [] };
const n = o5.xs.push(1, 2, 3);
console.log("field-push-returns-length", n, o5.xs[2]);

const o6: { xs: string[] } = { xs: [] };
o6.xs.push("a", "b");
console.log("field-push-strings", o6.xs.join("-"));

class Store {
  items: number[] = [];
}
const st = new Store();
st.items.push(1, 2);
console.log("instance-field-push", st.items.length, st.items[1]);

class Box<T> {
  items: T[] = [];
}
const b = new Box<number>();
b.items.push(1, 2);
console.log("generic-instance-push", b.items.map((x) => x * 2)[1]);

function mk(): { xs: number[] } {
  return { xs: [] };
}
const m = mk();
m.xs.push(4, 5);
console.log("declared-return-receiver", m.xs.length, m.xs[1]);

// The receiver is read once, not once per argument.
let reads = 0;
function src(): { xs: number[] } {
  reads = reads + 1;
  return holder;
}
const holder: { xs: number[] } = { xs: [] };
src().xs.push(1, 2, 3);
console.log("receiver-read-once", reads, holder.xs.length);

// Name receivers and single-argument calls, unchanged.
const xs: number[] = [];
xs.push(1, 2, 3);
xs.unshift(0);
console.log("name-receiver", xs.length, xs[0], xs[3]);

const one: { xs: number[] } = { xs: [] };
one.xs.push(7);
console.log("single-arg-field", one.xs[0], one.xs.length);
