#!/usr/bin/env node
// ES2023 §12.5 HashbangComment — `#!` runs to the end of the line, and is
// permitted only at the very start of the source text. Anywhere else a
// `#` is a private name or an error, which is why this is not a comment
// form the lexer recognises generally.
console.log("hashbang");

class C {
  #p = 1;
  read(): number {
    return this.#p;
  }
}
// A `#` after the first line still means what it always meant.
console.log(new C().read());
