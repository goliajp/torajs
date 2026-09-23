// A getter whose body falls through hands back the `undefined`
// sentinel. Reading it IN PLACE has answered right since the accessor
// arm landed; what this pins is the sentinel surviving the hop into a
// slot that was DECLARED — a typed parameter, or an array element.
//
// Two things have to happen for that, and only one of them is about
// the taint tables. The read has to be admitted as a sentinel source
// (a getter name is never WRITTEN, so the field set never saw it),
// and the slot it lands in has to be F64 — the sentinel is a NaN bit
// pattern, and an I64 slot cannot spell one, so arming an I64 slot is
// a silent no-op that looks exactly like the table having missed the
// value.
const zs: number[] = [1, 2, 3];
const src = { get g(): number { return zs[9]; } };

function show(p: number): void {
  console.log("param", typeof p, p === undefined);
}

// in place — the shape that already worked, kept as the control
console.log("direct", typeof src.g, src.g === undefined);

// through a binding, then a typed parameter
const u = src.g;
show(u);

// straight into a typed parameter
show(src.g);

// into an array element, read back through a callback
const ys: number[] = [src.g, 4];
ys.forEach((v) => console.log("elem", typeof v, v === undefined));

// a getter that does NOT fall through stays an ordinary number: the
// admission is per getter body, not per property name
const ok = { get h(): number { return zs[0]; } };
show(ok.h);
console.log("ok", typeof ok.h, ok.h === undefined);
