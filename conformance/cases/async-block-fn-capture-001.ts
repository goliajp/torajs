// A function declared in a block of an async body reads the body's
// locals, before and after an await (§14.2.3). It used to be lifted to
// top level with nothing to capture: `x is not defined`.

async function m() {
  let x = 5;
  await 0;
  {
    function h() { return x; }
    console.log(h());
  }
}

async function later(p: number) {
  let x = 5;
  await 0;
  {
    function h() { return x + p; }
    console.log(h());
    x = 7;
    console.log(h());
  }
  // initialized on block entry: a call above the text works
  {
    console.log(early());
    function early() { return "early " + x; }
  }
}

m()
  .then(() => later(2))
  .then(() => console.log("done"));
