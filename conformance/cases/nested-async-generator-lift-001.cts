// A nested `function*` lifts to the top level before the generator
// desugar claims its decls, and an ASYNC one used to be skipped: its
// channel is keyed by NAME in `async_generator_fns`, so the mangled
// decl read as a SYNC generator at a dozen sites downstream — the
// factory got Promise-wrapped, `.constructor` answered the wrong
// kind, and an earlier attempt recorded a SIGSEGV. Carrying the
// registry entry across the rename is what was missing; it is the
// same carry the generic-body clone already makes.
//
// Unchanged, and probed the same for both kinds — which is the
// point: a nested generator that CAPTURES an enclosing local fails
// (the lift moves the body to top level and the binding does not come
// with it), and one nested TWO levels deep fails (the rename does not
// reach through the middle body). Sync generators have answered that
// way all along; async ones now answer the same way instead of
// refusing every shape.
async function outerAsync() {
  async function* inner() {
    yield 1;
    yield 2;
  }
  var out = [];
  for await (var v of inner()) out.push(v);
  return out.join(",");
}

function outerPlain() {
  async function* inner() {
    yield 3;
  }
  return inner();
}

// A top-level async generator is untouched by any of this.
async function* topLevel() {
  yield 5;
}

async function main() {
  console.log("nested-async", await outerAsync());
  var out = [];
  for await (var v of outerPlain()) out.push(v);
  console.log("nested-plain", out.join(","));
  var t = [];
  for await (var w of topLevel()) t.push(w);
  console.log("top-level   ", t.join(","));
}
main();
