import { useEffect, useState } from 'react'

/**
 * torajs.com landing page.
 *
 * Aesthetic — deep ink (#0a0908) backdrop, bone (#f5f1e8) text,
 * tiger orange (#ff6f1a) accent. Editorial-meets-instrument vibe:
 * Fraunces serif for the wordmark + numerals, JetBrains Mono for
 * code + readouts, Inter Tight for prose. Single column down the
 * center, with thin amber rules separating sections — the visual
 * cadence is a magazine spread, not a SaaS tile grid.
 *
 * Content density modeled on bun.sh: hero + headline numbers +
 * inline install + "what you get" cards + perf chart + tabbed
 * code samples + status + footer. Each section answers the
 * "why should I install this?" question once more.
 *
 * Data is hardcoded against the bench scoreboard committed in this
 * repo (README.md headline numbers, `bench/results/*`). Refresh
 * `BENCH` below when the scoreboard publishes a new run.
 */
export function Landing() {
  return (
    <>
      <Header />
      <Hero />
      <PaintLine spacing="lg" />
      <WhyGrid />
      <PaintLine spacing="md" />
      <CodeShowcase />
      <PaintLine spacing="md" />
      <Bench />
      <PaintLine spacing="md" />
      <CapabilityGrid />
      <PaintLine spacing="md" />
      <ClosingNote />
      <Footer />
    </>
  )
}

/* ------------------------------------------------------------------ */
/* Header                                                             */
/* ------------------------------------------------------------------ */

function Header() {
  return (
    <header className="settle relative z-10 mx-auto flex max-w-[1080px] items-center justify-between px-6 pt-6 sm:pt-8">
      <a
        href="/"
        className="wordmark-roman text-bone text-[26px] tracking-tight"
        style={{ letterSpacing: '-0.05em' }}
      >
        <span className="text-tiger">tora</span>
        <span className="opacity-90">js</span>
      </a>

      <nav className="text-bone-dim hidden items-center gap-6 font-mono text-[11.5px] tracking-[0.18em] uppercase sm:flex">
        <a className="hover:text-tiger-bright transition-colors" href="#why">
          Why
        </a>
        <a className="hover:text-tiger-bright transition-colors" href="#code">
          Code
        </a>
        <a className="hover:text-tiger-bright transition-colors" href="#bench">
          Bench
        </a>
        <a className="hover:text-tiger-bright transition-colors" href="#status">
          Status
        </a>
        <a
          className="text-bone hover:text-tiger-bright transition-colors"
          href="https://github.com/goliajp/torajs"
        >
          GitHub ↗
        </a>
      </nav>
    </header>
  )
}

/* ------------------------------------------------------------------ */
/* Hero — wordmark + tagline + install + headline numbers all in     */
/* the first viewport (bun.sh-shaped density)                         */
/* ------------------------------------------------------------------ */

function Hero() {
  const cmd = 'curl -fsSL https://install.torajs.com | bash'
  const [copied, setCopied] = useState(false)
  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(cmd)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      /* no clipboard — silent fallback, the command stays selectable */
    }
  }

  return (
    <section className="relative mx-auto max-w-[1080px] px-6 pt-12 pb-10 sm:pt-20 sm:pb-16">
      <p className="eyebrow settle" style={{ animationDelay: '60ms' }}>
        v0.1.0-beta · TypeScript runtime · AOT to native
      </p>

      <h1
        className="wordmark settle text-bone mt-5 text-[20vw] leading-[0.82] sm:mt-7 sm:text-[180px]"
        style={{ animationDelay: '160ms' }}
      >
        <span className="text-tiger">tora</span>
        <span className="text-bone">js</span>
        <span className="text-tiger">.</span>
      </h1>

      <p
        className="settle text-bone-dim mt-8 max-w-[700px] text-[20px] leading-[1.45] sm:text-[26px]"
        style={{ animationDelay: '320ms', fontWeight: 350 }}
      >
        A TypeScript runtime that runs the same programs <Inline mark>bun</Inline> runs, with the
        same semantics — compiled ahead-of-time to a tiny native binary.
      </p>

      <div
        id="install"
        className="settle mt-10 grid gap-5 sm:mt-12 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center sm:gap-6"
        style={{ animationDelay: '440ms' }}
      >
        <button
          onClick={onCopy}
          className="code-block group relative overflow-hidden text-left transition-transform hover:-translate-y-px"
          aria-label="Copy install command"
        >
          <div className="border-rule text-bone-faint flex items-center justify-between border-b px-5 py-2 font-mono text-[10.5px] tracking-[0.2em] uppercase">
            <span>shell · macOS arm64 · Linux x64</span>
            <span className="group-hover:text-tiger-bright transition-colors">
              {copied ? 'copied ✓' : 'click to copy'}
            </span>
          </div>
          <pre className="px-5 py-5 font-mono text-[15.5px] leading-[1.55] sm:text-[17px]">
            <span className="text-bone-faint">$</span> <span className="text-bone">curl</span>{' '}
            <span className="tk-num">-fsSL</span>{' '}
            <span className="tk-str">https://install.torajs.com</span>{' '}
            <span className="text-bone">|</span> <span className="text-tiger">bash</span>
            <span className="cursor-blink" />
          </pre>
        </button>

        <div className="flex flex-wrap items-center gap-3">
          <a
            href="https://github.com/goliajp/torajs"
            className="border-rule text-bone hover:border-tiger hover:text-tiger-bright inline-flex items-center gap-2 border px-5 py-3 font-mono text-[11.5px] tracking-[0.18em] uppercase transition-colors"
          >
            GitHub
            <span aria-hidden>↗</span>
          </a>
          <a
            href="https://github.com/goliajp/torajs/blob/main/docs/getting-started.md"
            className="text-bone-dim hover:text-tiger-bright inline-flex items-center gap-2 px-2 py-3 font-mono text-[11.5px] tracking-[0.18em] uppercase transition-colors"
          >
            Docs →
          </a>
        </div>
      </div>

      <div
        className="settle border-rule/70 sm:divide-rule/70 mt-12 grid gap-4 border-y py-7 sm:mt-16 sm:grid-cols-4 sm:gap-0 sm:divide-x"
        style={{ animationDelay: '560ms' }}
      >
        <Stat label="Cold start" value="~1.3 ms" hint="tr run hello.ts" />
        <Stat label="Binary size" value="~40 KB" hint="tr build, statically linked" />
        <Stat label="Bench scoreboard" value="19 / 19" hint="cases tr build wins vs bun" />
        <Stat label="bun-parity" value="99.7 %" hint="of cases tr accepts" last />
      </div>

      <Sigil />
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* Decorative tiger-stripe glyph in the hero corner.                  */
/* ------------------------------------------------------------------ */

function Sigil() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 240 320"
      className="absolute top-12 right-6 hidden h-[300px] w-auto opacity-60 lg:block"
    >
      <g
        fill="none"
        stroke="currentColor"
        className="text-tiger"
        strokeWidth="1.2"
        strokeLinecap="round"
      >
        <path d="M40,20 L40,300" opacity="0.7" />
        <path d="M68,40 L68,290" opacity="0.45" />
        <path d="M96,8 L96,310" opacity="0.55" />
        <path d="M124,32 L124,300" opacity="0.35" />
        <path d="M152,16 L152,304" opacity="0.45" />
        <path d="M180,40 L180,288" opacity="0.3" />
        <path d="M20,160 L210,160" opacity="0.2" stroke="white" />
        <circle cx="40" cy="160" r="2.4" fill="currentColor" stroke="none" />
        <circle cx="96" cy="160" r="2.4" fill="currentColor" stroke="none" />
        <circle cx="152" cy="160" r="2.4" fill="currentColor" stroke="none" />
      </g>
    </svg>
  )
}

/* ------------------------------------------------------------------ */
/* Why grid — the bun.sh-style "what you get" 4-card pitch.           */
/* ------------------------------------------------------------------ */

const WHY: { tag: string; title: string; body: string; figure: string; figureLabel: string }[] = [
  {
    tag: '01',
    title: 'AOT to a real binary',
    body: 'Same compiler path serves `tr build` and `tr run`. No V8 bundle, no JIT, no GC pauses. Static, statically-linked, ready to drop into a container.',
    figure: '40 KB',
    figureLabel: 'tr build · static binary',
  },
  {
    tag: '02',
    title: 'Bun is the oracle',
    body: "Anything bun runs that tr accepts produces bun-identical output. When behavior is unclear, write the equivalent in TS, run it in bun, and match. Coverage grows; semantics don't drift.",
    figure: '99.67 %',
    figureLabel: 'of tr-accepted test262 cases match bun',
  },
  {
    tag: '03',
    title: 'Cold start measured in microseconds',
    body: 'No engine warm-up. From `exec` to first user line in ≈ 1.3 ms on Apple Silicon. The whole runtime is the binary you wrote.',
    figure: '1.3 ms',
    figureLabel: 'startup case · M4 Pro',
  },
  {
    tag: '04',
    title: 'TS as you actually write it',
    body: 'Classes, generics, closures, generators, try / catch, JSON, multi-file imports, the full string / array / Math / Number stdlib — implemented and verified, end-to-end.',
    figure: '301 / 301',
    figureLabel: 'three-way conformance · bun + tr-jit + tr-aot',
  },
]

function WhyGrid() {
  return (
    <section id="why" className="mx-auto max-w-[1080px] px-6 py-14 sm:py-20">
      <SectionLabel index="01" label="Why torajs" />

      <h2 className="wordmark-roman text-bone mt-6 max-w-[760px] text-[44px] leading-[0.95] sm:mt-8 sm:text-[64px]">
        TypeScript, <span className="text-tiger">compiled</span>{' '}
        <span className="text-bone">— not just transpiled.</span>
      </h2>
      <p className="text-bone-dim mt-6 max-w-[640px] text-[16.5px] leading-[1.55]">
        Most runtimes optimize the latency between source and JIT. tr skips the JIT entirely. Same
        TS, all the way down to a binary.
      </p>

      <ul className="border-rule/70 bg-rule/40 mt-12 grid gap-px overflow-hidden border sm:mt-16 sm:grid-cols-2">
        {WHY.map((item) => (
          <li
            key={item.tag}
            className="bg-ink-2/85 group hover:bg-ink-3 flex flex-col gap-6 px-6 py-8 transition-colors sm:px-8 sm:py-10"
          >
            <div className="flex items-center justify-between">
              <span className="text-tiger font-mono text-[11px] tracking-[0.2em] uppercase">
                {item.tag}
              </span>
              <span className="text-bone-faint font-mono text-[10.5px] tracking-[0.18em] uppercase">
                {item.figureLabel}
              </span>
            </div>
            <h3 className="wordmark-roman text-bone text-[28px] leading-[1.05] sm:text-[34px]">
              {item.title}
            </h3>
            <p className="text-bone-dim text-[14.5px] leading-[1.6]">{item.body}</p>
            <p className="font-display num text-tiger-bright mt-auto text-[40px] font-medium tracking-tight sm:text-[52px]">
              {item.figure}
            </p>
          </li>
        ))}
      </ul>
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* Code showcase — tabbed examples (sha256 / fizz-buzz / json)        */
/* ------------------------------------------------------------------ */

type TabKey = 'sha256' | 'fizzbuzz' | 'json'

function CodeShowcase() {
  const [tab, setTab] = useState<TabKey>('sha256')

  return (
    <section id="code" className="mx-auto max-w-[1080px] px-6 py-14 sm:py-20">
      <SectionLabel index="02" label="Real TypeScript" />

      <div className="mt-8 grid gap-12 sm:grid-cols-[1fr_1.15fr] sm:items-start sm:gap-16">
        <div>
          <h2 className="wordmark-roman text-bone text-[40px] leading-[0.95] sm:text-[56px]">
            What you write is what bun runs.
          </h2>
          <p className="text-bone-dim mt-6 max-w-[440px] text-[15.5px] leading-[1.6]">
            Every snippet on the right is a real example from the repo.
            <br />
            <br />
            Run them locally:
          </p>
          <pre className="text-bone-dim mt-4 font-mono text-[13px]">
            <span className="text-bone-faint">$</span> <span className="text-tiger">tr run</span>{' '}
            examples/sha256/sha256.ts
          </pre>
          <p className="text-bone-faint mt-8 font-mono text-[11px] tracking-[0.18em] uppercase">
            More in{' '}
            <a className="link-amber" href="https://github.com/goliajp/torajs/tree/main/examples">
              examples /
            </a>
          </p>
        </div>

        <div className="code-block flex flex-col overflow-hidden">
          <div className="border-rule flex items-center gap-1 border-b px-2">
            <Tab active={tab === 'sha256'} onClick={() => setTab('sha256')}>
              sha256.ts
            </Tab>
            <Tab active={tab === 'fizzbuzz'} onClick={() => setTab('fizzbuzz')}>
              fizz-buzz.ts
            </Tab>
            <Tab active={tab === 'json'} onClick={() => setTab('json')}>
              json-pretty.ts
            </Tab>
            <span className="text-tiger-bright ml-auto py-2 pr-3 font-mono text-[10.5px] tracking-[0.18em] uppercase">
              tr ↔ bun · parity
            </span>
          </div>
          <pre className="overflow-x-auto px-5 py-5 font-mono text-[12.5px] leading-[1.65] sm:text-[13.5px]">
            <code>
              {tab === 'sha256' && <Sha256Sample />}
              {tab === 'fizzbuzz' && <FizzBuzzSample />}
              {tab === 'json' && <JsonSample />}
            </code>
          </pre>
        </div>
      </div>
    </section>
  )
}

function Tab({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      onClick={onClick}
      className={`relative px-3 py-2 font-mono text-[11.5px] tracking-[0.16em] uppercase transition-colors ${
        active ? 'text-bone' : 'text-bone-faint hover:text-bone-dim'
      }`}
    >
      {children}
      {active && <span aria-hidden className="bg-tiger absolute right-3 bottom-0 left-3 h-[2px]" />}
    </button>
  )
}

function Sha256Sample() {
  return (
    <>
      <span className="tk-com">{`// SHA-256 — bit-twiddle heavy, JS UInt32 coercion via \`>>> 0\`.`}</span>
      {'\n'}
      <span className="tk-kw">function</span> <span className="tk-fn">rotr</span>(
      <span className="text-bone">x</span>: <span className="tk-typ">number</span>,{' '}
      <span className="text-bone">n</span>: <span className="tk-typ">number</span>):{' '}
      <span className="tk-typ">number</span> {`{`}
      {'\n'}
      {'  '}
      <span className="tk-kw">return</span> ((
      <span className="text-bone">x</span> <span className="tk-kw">{'>>>'}</span>{' '}
      <span className="text-bone">n</span>) | (<span className="text-bone">x</span>{' '}
      <span className="tk-kw">{'<<'}</span> (<span className="tk-num">32</span> -{' '}
      <span className="text-bone">n</span>))) <span className="tk-kw">{'>>>'}</span>{' '}
      <span className="tk-num">0</span>;{'\n'}
      {`}`}
      {'\n\n'}
      <span className="tk-kw">function</span> <span className="tk-fn">sha256Of</span>(
      <span className="text-bone">s</span>: <span className="tk-typ">string</span>):{' '}
      <span className="tk-typ">string</span> {`{`}
      {'\n'}
      {'  '}
      <span className="tk-kw">return</span> <span className="tk-fn">digestHex</span>(
      <span className="tk-fn">sha256Bytes</span>(<span className="tk-fn">strToBytes</span>(
      <span className="text-bone">s</span>)));{'\n'}
      {`}`}
      {'\n\n'}
      <span className="tk-com">{`// matches NIST FIPS 180-4 known-answer vectors.`}</span>
      {'\n'}
      <span className="text-bone">console</span>.<span className="tk-fn">log</span>(
      <span className="tk-fn">sha256Of</span>(<span className="tk-str">&quot;abc&quot;</span>));
      {'\n'}
      <span className="tk-com">{`// → ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`}</span>
    </>
  )
}

function FizzBuzzSample() {
  return (
    <>
      <span className="tk-com">{`// Classic FizzBuzz — number→string, modulo, conditional flow.`}</span>
      {'\n'}
      <span className="tk-kw">function</span> <span className="tk-fn">fizzBuzz</span>(
      <span className="text-bone">n</span>: <span className="tk-typ">number</span>):{' '}
      <span className="tk-typ">void</span> {`{`}
      {'\n'}
      {'  '}
      <span className="tk-kw">for</span> ( <span className="tk-kw">let</span>{' '}
      <span className="text-bone">i</span> = <span className="tk-num">1</span>;{' '}
      <span className="text-bone">i</span> {'<='} <span className="text-bone">n</span>;{' '}
      <span className="text-bone">i</span>++ ) {`{`}
      {'\n'}
      {'    '}
      <span className="tk-kw">if</span> (<span className="text-bone">i</span> %{' '}
      <span className="tk-num">15</span> === <span className="tk-num">0</span>){' '}
      <span className="text-bone">console</span>.<span className="tk-fn">log</span>(
      <span className="tk-str">&quot;FizzBuzz&quot;</span>);{'\n'}
      {'    '}
      <span className="tk-kw">else if</span> (<span className="text-bone">i</span> %{' '}
      <span className="tk-num">3</span> === <span className="tk-num">0</span>){' '}
      <span className="text-bone">console</span>.<span className="tk-fn">log</span>(
      <span className="tk-str">&quot;Fizz&quot;</span>);{'\n'}
      {'    '}
      <span className="tk-kw">else if</span> (<span className="text-bone">i</span> %{' '}
      <span className="tk-num">5</span> === <span className="tk-num">0</span>){' '}
      <span className="text-bone">console</span>.<span className="tk-fn">log</span>(
      <span className="tk-str">&quot;Buzz&quot;</span>);{'\n'}
      {'    '}
      <span className="tk-kw">else</span> <span className="text-bone">console</span>.
      <span className="tk-fn">log</span>(<span className="text-bone">i</span>.
      <span className="tk-fn">toString</span>());{'\n'}
      {'  '}
      {`}`}
      {'\n'}
      {`}`}
      {'\n\n'}
      <span className="tk-fn">fizzBuzz</span>(<span className="tk-num">20</span>);
    </>
  )
}

function JsonSample() {
  return (
    <>
      <span className="tk-com">{`// Class instances → JSON, parse round-trip with type inference.`}</span>
      {'\n'}
      <span className="tk-kw">class</span> <span className="tk-typ">User</span> {`{`}
      {'\n'}
      {'  '}
      <span className="text-bone">name</span>: <span className="tk-typ">string</span>;{'\n'}
      {'  '}
      <span className="text-bone">tags</span>: <span className="tk-typ">string</span>[];{'\n'}
      {'  '}
      <span className="tk-fn">constructor</span>(<span className="text-bone">name</span>:{' '}
      <span className="tk-typ">string</span>, <span className="text-bone">tags</span>:{' '}
      <span className="tk-typ">string</span>[]) {`{`}
      {'\n'}
      {'    '}
      <span className="tk-kw">this</span>.<span className="text-bone">name</span> ={' '}
      <span className="text-bone">name</span>; <span className="tk-kw">this</span>.
      <span className="text-bone">tags</span> = <span className="text-bone">tags</span>;{'\n'}
      {'  '}
      {`}`}
      {'\n'}
      {`}`}
      {'\n\n'}
      <span className="tk-kw">const</span> <span className="text-bone">alice</span> ={' '}
      <span className="tk-kw">new</span> <span className="tk-typ">User</span>(
      <span className="tk-str">&quot;Alice&quot;</span>, [
      <span className="tk-str">&quot;admin&quot;</span>,{' '}
      <span className="tk-str">&quot;engineer&quot;</span>]);{'\n'}
      <span className="text-bone">console</span>.<span className="tk-fn">log</span>(
      <span className="text-bone">JSON</span>.<span className="tk-fn">stringify</span>(
      <span className="text-bone">alice</span>));{'\n'}
      <span className="tk-com">{`// → {"name":"Alice","tags":["admin","engineer"]}`}</span>
      {'\n\n'}
      <span className="tk-com">{`// caller-driven type inference for the parsed shape:`}</span>
      {'\n'}
      <span className="tk-kw">const</span> <span className="text-bone">arr</span>:{' '}
      <span className="tk-typ">number</span>[] = <span className="text-bone">JSON</span>.
      <span className="tk-fn">parse</span>(<span className="tk-str">&apos;[10, 20, 30]&apos;</span>
      );
    </>
  )
}

/* ------------------------------------------------------------------ */
/* Bench scoreboard                                                   */
/* ------------------------------------------------------------------ */

type BenchRow = {
  case: string
  tr: number
  rust: number
  go: number
  bun: number
  highlight?: 'tr' | 'rust' | 'go' | 'bun'
  label?: string
}

const BENCH: BenchRow[] = [
  { case: 'fib40', tr: 147.25, rust: 181.77, go: 232.51, bun: 380.58, highlight: 'tr' },
  { case: 'collatz', tr: 106.41, rust: 106.13, go: 143.43, bun: 328.14, highlight: 'rust' },
  { case: 'closure-pipeline-1m', tr: 16.68, rust: 19.24, go: 35.53, bun: 47.65, highlight: 'tr' },
  { case: 'array-sum-1m', tr: 11.53, rust: 13.82, go: 31.5, bun: 50.78, highlight: 'tr' },
  { case: 'gcd1m', tr: 40.48, rust: 41.01, go: 41.32, bun: 50.33, highlight: 'tr' },
  { case: 'popcount', tr: 2.64, rust: 2.73, go: 52.25, bun: 58.67, highlight: 'tr' },
  { case: 'startup', tr: 1.41, rust: 1.49, go: 1.89, bun: 8.25, highlight: 'tr' },
  {
    case: 'throw-catch-100k',
    tr: 1.29,
    rust: 426.23,
    go: 7.32,
    bun: 23.6,
    highlight: 'tr',
    label: '330× rust',
  },
]

function Bench() {
  /* IntersectionObserver triggers the bar fills on scroll into view.
   * Default to true if the API isn't available so non-IO environments
   * still show the chart. */
  const [visible, setVisible] = useState(() => typeof IntersectionObserver === 'undefined')

  useEffect(() => {
    if (visible) return
    const el = document.getElementById('bench')
    if (!el) return
    const obs = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true)
          obs.disconnect()
        }
      },
      { threshold: 0.18 }
    )
    obs.observe(el)
    return () => obs.disconnect()
  }, [visible])

  return (
    <section id="bench" className="scanlines mx-auto max-w-[1080px] px-6 py-16 sm:py-24">
      <SectionLabel index="03" label="Bench scoreboard" />

      <div className="mt-8 grid gap-10 sm:grid-cols-[1fr_auto] sm:items-end sm:gap-12">
        <div>
          <h2 className="wordmark-roman text-bone text-[40px] leading-[0.95] sm:text-[56px]">
            <span className="text-tiger">19/19</span>
            <span> bench cases, tr build wins.</span>
          </h2>
          <p className="text-bone-dim mt-5 max-w-[600px] text-[15.5px] leading-[1.6]">
            Cross-runtime perf, Apple M4 Pro, hyperfine n=10 with 3 warmup runs. Eight
            representative rows below — full cross-runtime table at{' '}
            <a className="link-amber" href="/bench">
              /bench
            </a>{' '}
            (auto-rendered from <code className="font-mono">bench/results/*.json</code>).
          </p>
        </div>
        <div className="text-bone-faint font-mono text-[11px] tracking-[0.2em] uppercase">
          ms · lower is better
        </div>
      </div>

      <ul className="mt-10 space-y-4 sm:mt-14">
        {BENCH.map((row, i) => {
          const max = Math.max(row.tr, row.rust, row.go, row.bun)
          return (
            <li
              key={row.case}
              className="grid gap-2 sm:grid-cols-[200px_1fr_120px] sm:items-center sm:gap-8"
            >
              <div className="text-bone font-mono text-[13px]">
                {row.case}
                {row.label && (
                  <span className="text-tiger-bright ml-2 text-[10.5px] tracking-[0.16em] uppercase">
                    {row.label}
                  </span>
                )}
              </div>
              <div className="space-y-1.5">
                <Bar
                  label="tr"
                  value={row.tr}
                  max={max}
                  visible={visible}
                  delay={i * 80}
                  tone="tiger"
                />
                <Bar
                  label="rust"
                  value={row.rust}
                  max={max}
                  visible={visible}
                  delay={i * 80 + 30}
                  tone="bone"
                />
                <Bar
                  label="go"
                  value={row.go}
                  max={max}
                  visible={visible}
                  delay={i * 80 + 60}
                  tone="bone-faint"
                />
                <Bar
                  label="bun"
                  value={row.bun}
                  max={max}
                  visible={visible}
                  delay={i * 80 + 90}
                  tone="bone-faint"
                />
              </div>
              <div className="num text-bone font-mono text-[13px] sm:text-right">
                <span className="text-tiger-bright">{row.tr.toFixed(2)}</span>{' '}
                <span className="text-bone-faint">ms</span>
              </div>
            </li>
          )
        })}
      </ul>
    </section>
  )
}

function Bar({
  label,
  value,
  max,
  visible,
  delay,
  tone,
}: {
  label: string
  value: number
  max: number
  visible: boolean
  delay: number
  tone: 'tiger' | 'bone' | 'bone-faint'
}) {
  const pct = max === 0 ? 0 : value / max
  const fill = tone === 'tiger' ? 'bg-tiger' : tone === 'bone' ? 'bg-bone-dim' : 'bg-bone-faint/60'
  return (
    <div className="grid grid-cols-[44px_1fr_60px] items-center gap-3">
      <span className="text-bone-faint font-mono text-[11px] tracking-[0.16em] uppercase">
        {label}
      </span>
      <div className="bg-rule/60 relative h-[5px] overflow-hidden">
        {visible && (
          <span
            className={`bar-fill absolute inset-y-0 left-0 ${fill}`}
            style={{
              width: `${pct * 100}%`,
              animationDelay: `${delay}ms`,
            }}
          />
        )}
      </div>
      <span className="num text-bone-dim font-mono text-[11px] tabular-nums">
        {value.toFixed(2)}
      </span>
    </div>
  )
}

/* ------------------------------------------------------------------ */
/* Capability grid — what works today                                 */
/* ------------------------------------------------------------------ */

const CAPABILITIES: { title: string; body: string }[] = [
  {
    title: 'Classes & generics',
    body: 'Instance + static, inheritance, abstract, visibility modifiers. Generics monomorphized per call site.',
  },
  {
    title: 'Closures · generators',
    body: 'Lifted closures with implicit captures. function*, yield, yield * — full state-machine lowering.',
  },
  {
    title: 'try / catch / finally',
    body: 'Module-level throw_active flag; throw is ~zero-cost when it doesn’t fire. throw-catch-100k: 1.3 ms.',
  },
  {
    title: 'JSON · multi-file imports',
    body: 'JSON.parse with caller-driven type inference. Cross-file named imports with cached compile.',
  },
  {
    title: 'Full string · array · Math',
    body: 'slice / repeat / replace / pad·, push / map / filter / reduce / sort, every Math.* + constants.',
  },
  {
    title: 'AOT to native, by default',
    body: 'tr build emits a real binary. tr run caches at ~/.torajs/cache so the dev loop is free after the first compile.',
  },
]

function CapabilityGrid() {
  return (
    <section id="status" className="mx-auto max-w-[1080px] px-6 py-14 sm:py-20">
      <SectionLabel index="04" label="What works today" />

      <div className="mt-8 grid gap-10 sm:grid-cols-[1.05fr_1fr] sm:gap-14">
        <h2 className="wordmark-roman text-bone text-[40px] leading-[0.95] sm:text-[52px]">
          Most everyday TS, already shipping.
        </h2>
        <p className="text-bone-dim text-[15.5px] sm:pt-3">
          The line is moving — not a frozen cut-down language. Anything bun runs that tr rejects is
          a roadmap-phase gap, not a permanent decision. Full feature table in{' '}
          <a
            className="link-amber"
            href="https://github.com/goliajp/torajs/blob/main/docs/language-status.md"
          >
            docs / language-status.md
          </a>
          .
        </p>
      </div>

      <ul className="mt-12 grid gap-x-10 gap-y-8 sm:grid-cols-2 lg:grid-cols-3">
        {CAPABILITIES.map((c, i) => (
          <li key={c.title} className="border-rule/70 border-l pl-5">
            <p className="eyebrow">{String(i + 1).padStart(2, '0')}</p>
            <h3 className="text-bone font-display mt-3 text-[20px] leading-[1.15] font-medium">
              {c.title}
            </h3>
            <p className="text-bone-dim mt-2 text-[14px] leading-[1.6]">{c.body}</p>
          </li>
        ))}
      </ul>
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* Closing note                                                        */
/* ------------------------------------------------------------------ */

function ClosingNote() {
  return (
    <section className="mx-auto max-w-[1080px] px-6 py-16 sm:py-24">
      <div className="grid gap-10 sm:grid-cols-[auto_1fr] sm:items-center sm:gap-14">
        <p
          aria-hidden
          className="wordmark text-tiger leading-[0.85]"
          style={{ fontSize: 'clamp(110px, 14vw, 188px)' }}
        >
          ⤳
        </p>
        <div>
          <h2 className="wordmark-roman text-bone text-[40px] leading-[0.95] sm:text-[60px]">
            Bun is the oracle.
          </h2>
          <p className="text-bone-dim mt-6 max-w-[560px] text-[16px] leading-[1.6]">
            When behavior is unclear, write the equivalent in TS, run it in{' '}
            <Inline mono>bun</Inline>, and match. If torajs differs from bun&rsquo;s output
            (excluding the documented perf differentiators), that&rsquo;s a bug — file an issue.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-5">
            <a
              className="text-tiger-bright font-mono text-[12px] tracking-[0.18em] uppercase"
              href="https://github.com/goliajp/torajs/issues/new"
            >
              File an issue →
            </a>
            <a
              className="text-bone-dim hover:text-bone font-mono text-[12px] tracking-[0.18em] uppercase transition-colors"
              href="https://github.com/goliajp/torajs/blob/main/docs/getting-started.md"
            >
              Read the docs →
            </a>
            <a
              className="text-bone-dim hover:text-bone font-mono text-[12px] tracking-[0.18em] uppercase transition-colors"
              href="https://github.com/goliajp/torajs/tree/main/examples"
            >
              Browse examples →
            </a>
          </div>
        </div>
      </div>
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* Footer                                                              */
/* ------------------------------------------------------------------ */

function Footer() {
  return (
    <footer className="border-rule/70 mt-12 border-t">
      <div className="mx-auto grid max-w-[1080px] gap-8 px-6 py-10 sm:grid-cols-[1.4fr_1fr_1fr_1fr]">
        <div>
          <p className="wordmark-roman text-bone text-[28px]">
            <span className="text-tiger">tora</span>
            <span>js</span>
          </p>
          <p className="text-bone-faint mt-3 text-[13px] leading-[1.5]">
            Ship the same TypeScript bun runs, faster — at a fraction of the size.
          </p>
        </div>
        <FootCol
          heading="Project"
          links={[
            ['GitHub', 'https://github.com/goliajp/torajs'],
            ['Issues', 'https://github.com/goliajp/torajs/issues'],
            ['Releases', 'https://github.com/goliajp/torajs/releases'],
          ]}
        />
        <FootCol
          heading="Docs"
          links={[
            [
              'Getting started',
              'https://github.com/goliajp/torajs/blob/main/docs/getting-started.md',
            ],
            [
              'Language status',
              'https://github.com/goliajp/torajs/blob/main/docs/language-status.md',
            ],
            ['Performance', 'https://github.com/goliajp/torajs/blob/main/docs/perf.md'],
          ]}
        />
        <FootCol
          heading="Examples"
          links={[
            ['SHA-256', 'https://github.com/goliajp/torajs/tree/main/examples/sha256'],
            ['Prime sieve', 'https://github.com/goliajp/torajs/tree/main/examples/prime-sieve'],
            ['JSON demo', 'https://github.com/goliajp/torajs/tree/main/examples/json-pretty'],
          ]}
        />
      </div>
      <div className="border-rule/40 border-t">
        <div className="mx-auto flex max-w-[1080px] flex-wrap items-center justify-between gap-2 px-6 py-4 font-mono text-[10.5px] tracking-[0.18em] uppercase">
          <span className="text-bone-faint">© torajs · Apache-2.0 · v0.1.0-beta</span>
          <span className="text-bone-faint">
            <span className="text-tiger">●</span> released 2026
          </span>
        </div>
      </div>
    </footer>
  )
}

/* ------------------------------------------------------------------ */
/* Small composable bits                                              */
/* ------------------------------------------------------------------ */

function FootCol({ heading, links }: { heading: string; links: [string, string][] }) {
  return (
    <div>
      <p className="eyebrow">{heading}</p>
      <ul className="mt-3 space-y-2">
        {links.map(([label, href]) => (
          <li key={href}>
            <a
              href={href}
              className="text-bone-dim hover:text-tiger-bright text-[13.5px] transition-colors"
            >
              {label}
            </a>
          </li>
        ))}
      </ul>
    </div>
  )
}

function SectionLabel({ index, label }: { index: string; label: string }) {
  return (
    <div className="flex items-baseline gap-4">
      <span className="num text-tiger font-mono text-[12px]">{index}</span>
      <span className="bg-rule h-[1px] max-w-[120px] flex-1" />
      <span className="eyebrow">{label}</span>
    </div>
  )
}

function PaintLine({ spacing }: { spacing: 'sm' | 'md' | 'lg' }) {
  const cls = spacing === 'lg' ? 'py-12' : spacing === 'md' ? 'py-8' : 'py-4'
  return (
    <div className={`mx-auto max-w-[1080px] px-6 ${cls}`}>
      <div className="tiger-rule" />
    </div>
  )
}

function Inline({
  children,
  mono,
  mark,
}: {
  children: React.ReactNode
  mono?: boolean
  mark?: boolean
}) {
  if (mono) {
    return <span className="text-tiger-bright font-mono text-[0.92em]">{children}</span>
  }
  if (mark) {
    return (
      <span className="text-bone decoration-tiger underline decoration-2 underline-offset-[6px]">
        {children}
      </span>
    )
  }
  return <span>{children}</span>
}

function Stat({
  label,
  value,
  hint,
  last,
}: {
  label: string
  value: string
  hint: string
  last?: boolean
}) {
  return (
    <div className={`px-0 sm:px-6 ${last ? '' : ''} ${last ? 'sm:pr-0' : ''} sm:first:pl-0`}>
      <p className="text-bone-faint font-mono text-[10.5px] tracking-[0.18em] uppercase">{label}</p>
      <p className="text-bone num font-display mt-2 text-[28px] font-medium tracking-tight sm:text-[32px]">
        {value}
      </p>
      <p className="text-bone-faint/80 mt-1 font-mono text-[10.5px] tracking-[0.16em] uppercase">
        {hint}
      </p>
    </div>
  )
}
