// RFC 20260727 — reading a promise back into a typed lane.
//
// A promise cell's value slot is repr-blind, but the stamp beside it
// records the storage form. The typed `.then` / `.catch` kernels only
// ever read that stamp in ONE direction — boxing a typed value for an
// `(v: any)` handler. The mirror case, a cell settled FROM an `any`
// handed to a handler that declared a typed lane, fell through to
// "pass the value along untouched" — so the NaN-box pointer was
// handed over raw and read as whatever the lane expected:
//
//   (v: number) => …   bitcast a pointer into an f64   → NaN
//   (v: string) => …   dereferenced it as a Str        → SIGSEGV
//
// The `(v: any)` reading of the very same cell answered correctly the
// whole time, which is how we know the payload was stored right and
// only the read-back was wrong.

function numLane() {
  const l: any = 42;
  const p: Promise<number> = Promise.resolve(l);
  p.then((v: number) => {
    console.log("num", v);
  });
}

// the annotation on `p` is not the discriminator — the handler's
// parameter type is, so the inferred form must behave identically
function numLaneNoAnn() {
  const l: any = 42;
  const p = Promise.resolve(l);
  p.then((v: number) => {
    console.log("num-inferred", v);
  });
}

function strLane() {
  const l: any = "s";
  const p: Promise<string> = Promise.resolve(l);
  p.then((v: string) => {
    console.log("str", v);
  });
}

function boolLane() {
  const l: any = true;
  const p: Promise<boolean> = Promise.resolve(l);
  p.then((v: boolean) => {
    console.log("bool", v);
  });
}

// the any-param reading of the same cell — correct before and after
function anyLane() {
  const l: any = 42;
  const p: Promise<number> = Promise.resolve(l);
  p.then((v: any) => {
    console.log("any", v);
  });
}

function twoArg() {
  const l: any = 7;
  const p: Promise<number> = Promise.resolve(l);
  p.then(
    (v: number) => {
      console.log("two-ok", v);
      return v;
    },
    (e: number) => {
      console.log("two-err", e);
      return e;
    },
  );
}

function catchStr() {
  const l: any = "boom";
  const p: Promise<string> = Promise.reject(l);
  p.catch((e: string) => {
    console.log("catch-str", e);
  });
}

function catchNum() {
  const l: any = 5;
  const p: Promise<number> = Promise.reject(l);
  p.catch((e: number) => {
    console.log("catch-num", e);
  });
}

// two hops — the result cell's stamp is written by the kernel, so the
// second handler must still find a form it can read
function chained() {
  const l: any = 3;
  const p: Promise<number> = Promise.resolve(l);
  p.then((v: number) => v * 2).then((v: number) => {
    console.log("chain", v);
  });
}

numLane();
numLaneNoAnn();
strLane();
boolLane();
anyLane();
twoArg();
catchStr();
catchNum();
chained();
