// §10.4.5's TypedArray and §25.3's DataView each own the ArrayBuffer
// they read through — the reference their own destructor releases at
// the `+8` word. Rotation 528 gave all three buffer-family tags their
// property bag, and that bag was the only child the walk enumerated,
// so `const v = new DataView(b); (b as any).v = v` was a two-cell
// ring nothing could reach.
//
// The buffer's bytes are not a cell and stay the destructor's; only
// the view -> buffer edge is new here.
//
// Observable here: the walk crosses these rings without crashing and
// a live one still reads through to the same bytes. Whether the dead
// ones are reclaimed is a peak-RSS question, measured out of band.

let ok = 0
for (let i = 0; i < 200; i++) {
  // buffer edge: dataview -> buffer -> dataview
  const b = new ArrayBuffer(8)
  const v: any = new DataView(b)
  ;(b as any).v = v
  if ((b as any).v === v) ok++

  // same shape through a typed array
  const b2 = new ArrayBuffer(8)
  const t: any = new Uint8Array(b2)
  ;(b2 as any).t = t
  if ((b2 as any).t === t) ok++

  // two views over one buffer, each reachable from it
  const b3 = new ArrayBuffer(16)
  const va: any = new DataView(b3)
  const vb: any = new Int32Array(b3)
  ;(b3 as any).pair = [va, vb]
  if ((b3 as any).pair[0] === va && (b3 as any).pair[1] === vb) ok++

  // through an object the buffer's expando holds
  const b4 = new ArrayBuffer(8)
  const holder: any = { view: new Float64Array(b4) }
  ;(b4 as any).h = holder
  if ((b4 as any).h.view.buffer === b4) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held ring still reads through to the same bytes.
const live = new ArrayBuffer(4)
const lv: any = new Uint8Array(live)
;(live as any).lv = lv
Bun.gc(true)
lv[0] = 9
console.log((live as any).lv[0], (live as any).lv === lv, lv.buffer === live)
