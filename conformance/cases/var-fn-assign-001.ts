// RFC 20260729-fn-value-any — the assign axis's `var` case, the
// mirror of V4 knife 3's init case.
//
// A `var` slot is an `any` destination whatever the source says: the
// hoist pass, which runs after the wrap collector, mints every
// hoisted binding as `any` on purpose — a pre-init read is
// `undefined`, and a var may be reassigned across types. Knife 3
// taught the INIT axis that (`var b = foo`); the ASSIGN axis still
// read only the written annotation, so a fn value stored into a var
// by a later assignment reached `box_to_any` as a raw FnSig and
// panicked the whole program. `let x: any; x = foo` had always
// worked, which is the same destination spelled out loud.

function foo(): number {
  return 1;
}
function bar(): number {
  return 2;
}

// declared with no initializer, assigned later — the shape the
// annexB block-decl cases carry
var later;
later = foo;
console.log(later(), typeof later);

// inside a fn body; the hoist walks each FnDecl body separately
function inBody(): number {
  var local;
  local = foo;
  return local();
}
console.log(inBody());

// initialized with something else first, then assigned a fn value
var reassigned = 0;
reassigned = foo;
console.log(reassigned());

// assigned twice, to two different fn values
var twice;
twice = foo;
console.log(twice());
twice = bar;
console.log(twice());

// the value keeps its identity as a callable through the slot
var named;
named = bar;
console.log(named.name, named.length);
