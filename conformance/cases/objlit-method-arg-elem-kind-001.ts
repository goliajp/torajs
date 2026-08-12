// r381 — an object literal's method read its array argument from a
// different element class than the caller filled. The direct-call arm
// and the F1 indirect arm both JOIN a container argument onto the
// callee's param key (RFC 20260726-array-elem-width); F5, the
// member-callee mirror, only handed over the scalar width. So an
// `any`-held block reaching `ns.take(...)` was read by a raw f64
// loader and every element answered NaN — silently, since `.length`
// (a header read) stayed right.

const y: any = [1, 2, 3];

const ns = {
  take(xs: number[]) {
    console.log("objlit", xs.length, xs[0], xs[1] + xs[2]);
  },
};
ns.take(y as number[]);

// the same block through a nested literal's method
const outer = {
  inner: {
    sum(xs: number[]) {
      let t = 0;
      for (const v of xs) t += v;
      console.log("nested", t);
    },
  },
};
outer.inner.sum(y as number[]);

// a second param position, and a scalar beside the container
const two = {
  go(tag: string, xs: number[]) {
    console.log("two", tag, xs[2]);
  },
};
two.go("t", y as number[]);

// strings behind `any` reach a string-element param the same way
const s: any = ["a", "b"];
const st = {
  join2(xs: string[]) {
    console.log("str", xs[0] + xs[1]);
  },
};
st.join2(s as string[]);

// a genuinely typed caller keeps the typed element lane — the join is
// evidence-driven, not a blanket widen
const pure = {
  take(xs: number[]) {
    console.log("pure", xs[0], xs[1] + xs[2]);
  },
};
pure.take([4, 5, 6]);

// writes through the method see the caller's block
const w: any = [7, 8];
const mut = {
  bump(xs: number[]) {
    xs[0] = xs[0] + 1;
    console.log("bump", xs[0], xs[1]);
  },
};
mut.bump(w as number[]);
console.log("after", w[0], w[1]);
