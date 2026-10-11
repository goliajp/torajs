// toLocaleString's locales / options arguments. Every structurally valid
// language tag is accepted; the runtime's only locale data is en-US (also
// the default locale), so ECMA-402 LookupMatcher resolves any tag it does
// not ship (de-DE, en-GB) to en-US, and an option the formatter does not
// apply yet is read and ignored. bun ships full locale data, so its
// answers for de-DE and minimumFractionDigits differ: this file keeps the
// expected output. Array and TypedArray toLocaleString read neither
// argument: each element's toLocaleString is invoked with no arguments
// (ES 23.1.3.32).

function probe(label: string, f: () => string): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "throws", e instanceof RangeError ? "RangeError" : e instanceof TypeError ? "TypeError" : "other");
  }
}

const n: number = 1234.5;
const i: number = 1234567;
const big: bigint = 12345678901234567890n;
const d: Date = new Date(2020, 0, 2, 3, 4, 5);
const bad: Date = new Date(NaN);
const xs: number[] = [1234.5, 2];
const ss: string[] = ["a", "b"];
const empty: number[] = [];

// en-US itself: no argument, undefined, en / en-US in any case
probe("num bare", () => n.toLocaleString());
probe("num en-US", () => n.toLocaleString("en-US"));
probe("num en", () => i.toLocaleString("en"));
probe("num EN-us", () => n.toLocaleString("EN-us"));
probe("num undefined", () => n.toLocaleString(undefined));
probe("num list", () => n.toLocaleString(["en-US", "de-DE"]));
probe("num empty list", () => n.toLocaleString([]));
probe("num empty options", () => n.toLocaleString("en-US", {}));
probe("num unread option", () => n.toLocaleString("en-US", { timeZone: "UTC" } as any));
probe("big en-US", () => big.toLocaleString("en-US"));
probe("date en-US", () => d.toLocaleDateString("en-US"));
probe("arr en-US", () => xs.toLocaleString("en-US"));
probe("str arr de-DE", () => ss.toLocaleString("de-DE"));
probe("empty arr de-DE", () => JSON.stringify(empty.toLocaleString("de-DE")));
probe("invalid date de-DE", () => bad.toLocaleString("de-DE"));

// other valid tags fall back to en-US, options are read and ignored;
// a malformed tag and null are refused
probe("num de-DE", () => n.toLocaleString("de-DE"));
probe("num list de first", () => n.toLocaleString(["de-DE", "en-US"]));
probe("num option", () => n.toLocaleString("en-US", { minimumFractionDigits: 2 }));
probe("num malformed", () => n.toLocaleString("x_y!"));
probe("num null", () => n.toLocaleString(null as any));
probe("big de-DE", () => big.toLocaleString("de-DE"));
probe("date de-DE", () => d.toLocaleString("de-DE"));
probe("date option", () => d.toLocaleTimeString("en-US", { hour12: false }));

// arrays hand their elements no arguments and never read the options
probe("arr de-DE", () => xs.toLocaleString("de-DE"));
probe("arr null", () => xs.toLocaleString(null as any));
let arrReads = 0;
const arrOpts = { get minimumFractionDigits() { arrReads++; return 2; } };
probe("arr option", () => xs.toLocaleString("en-US", arrOpts as any) + " " + arrReads);
const seen: any[] = [];
const spy: any = { toLocaleString(...a: any[]) { seen.push(a.length); return "spy"; } };
const mixed: any[] = [spy, 1234.5, spy];
probe("element args", () => mixed.toLocaleString("de-DE", { style: "percent" }) + " " + seen.join("/"));
let argEvals = 0;
const ev = (): string => { argEvals++; return "de-DE"; };
probe("arg evaluated", () => xs.toLocaleString(ev()) + " " + mixed.toLocaleString(ev()) + " " + argEvals);

// the any lane answers the same
const a: any = 1234.5;
const ad: any = d;
const aa: any = xs;
probe("any num en-US", () => a.toLocaleString("en-US"));
probe("any num de-DE", () => a.toLocaleString("de-DE"));
probe("any num option", () => a.toLocaleString(undefined, { style: "percent" }));
probe("any date de-DE", () => ad.toLocaleDateString("de-DE"));
probe("any arr de-DE", () => aa.toLocaleString("de-DE"));
probe("any arr option", () => aa.toLocaleString("en-US", arrOpts) + " " + arrReads);
probe("typed array de-DE", () => (new Float64Array([1234.5]) as any).toLocaleString("de-DE"));
probe("typed array option", () => (new Int32Array([1234]) as any).toLocaleString("de-DE", arrOpts) + " " + arrReads);

// each option getter runs once; a primitive options value carries no
// option names
let reads = 0;
const counted = { get minimumFractionDigits() { reads++; return undefined; } };
probe("getter once", () => n.toLocaleString("en-US", counted as any) + " " + reads);
probe("de with null options", () => n.toLocaleString("de", null as any));
probe("primitive options", () => n.toLocaleString("en-US", 5 as any));
probe("non-string locale element", () => n.toLocaleString([5] as any));
probe("call en-US", () => Number.prototype.toLocaleString.call(1234.5, "en-US"));
probe("call de", () => Number.prototype.toLocaleString.call(1234.5, "de"));
probe("wrapper en-US", () => new Number(1234.5).toLocaleString("en-US"));
