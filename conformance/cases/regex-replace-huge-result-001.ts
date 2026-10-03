// A `$` replacement can describe a result far longer than any string:
// each `$1` repeats a 1 MiB capture, so 65536 of them ask for 64 GiB.
// The replace raises a RangeError the program can catch (the
// implementation limit on string length) before allocating anything.

var x = "1".repeat(1 << 20);

function attempt(tag: string, f: () => string) {
  try {
    console.log(tag, f().length);
  } catch (e) {
    console.log(tag, "caught", e instanceof RangeError);
  }
}

attempt("group", () => x.replace(/(.+)/g, "$1".repeat(1 << 16)));
attempt("match", () => x.replace(/.+/, "$&".repeat(1 << 16)));
attempt("all", () => x.replaceAll(/1+/g, "$&".repeat(1 << 16)));
attempt("sticky", () => x.replace(/(1+)/y, "[$1]".repeat(1 << 16)));

// the same replace with a result that fits still answers it
attempt("fits", () => x.replace(/(.+)/g, "$1".repeat(4)));
attempt("after", () => "abc".replace(/b/, "$&$&"));
