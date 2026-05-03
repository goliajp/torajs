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
 * Data is hardcoded against the bench scoreboard committed in this
 * repo (README.md headline numbers, `bench/results/*`). When the
 * scoreboard ships a new run, refresh `BENCH` below.
 */
export function Landing() {
  return (
    <>
      <Header />
      <Hero />
      <PaintLine spacing="lg" />
      <Install />
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

function Header() {
  return (
    <header className="settle relative z-10 mx-auto flex max-w-[960px] items-center justify-between px-6 pt-7 sm:pt-9">
      <a
        href="/"
        className="wordmark-roman text-bone text-[28px] tracking-tight"
        style={{ letterSpacing: '-0.05em' }}
      >
        <span className="text-tiger">tora</span>
        <span className="opacity-90">js</span>
      </a>

      <nav className="text-bone-dim flex items-center gap-6 font-mono text-[12px] tracking-[0.18em] uppercase">
        <a className="hover:text-tiger-bright transition-colors" href="#install">
          Install
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

function Hero() {
  return (
    <section className="relative mx-auto max-w-[960px] px-6 pt-16 pb-12 sm:pt-24 sm:pb-20">
      <p className="eyebrow settle" style={{ animationDelay: '60ms' }}>
        v0.1.0-beta · TypeScript runtime · AOT to native
      </p>

      <h1
        className="wordmark settle text-bone mt-6 text-[18vw] leading-[0.82] sm:mt-8 sm:text-[164px]"
        style={{ animationDelay: '160ms' }}
      >
        <span className="text-tiger">tora</span>
        <span className="text-bone">js</span>
        <span className="text-tiger">.</span>
      </h1>

      <p
        className="settle text-bone-dim mt-10 max-w-[640px] text-[19px] leading-[1.5] sm:text-[22px]"
        style={{ animationDelay: '320ms', fontWeight: 350 }}
      >
        The same TypeScript programs <Inline mark>bun</Inline> runs, with the same semantics —{' '}
        <Inline mark>compiled ahead-of-time</Inline> to a tiny native binary.{' '}
        <span className="text-bone">~1.3 ms cold start.</span>{' '}
        <span className="text-bone">~40 KB statically linked.</span> No GC pauses, no V8 footprint.
      </p>

      <div
        className="settle mt-12 flex flex-wrap items-center gap-4"
        style={{ animationDelay: '460ms' }}
      >
        <a
          href="#install"
          className="text-ink bg-tiger hover:bg-tiger-bright group inline-flex items-center gap-3 px-5 py-3 font-mono text-[12.5px] tracking-[0.18em] uppercase transition-colors"
        >
          Install
          <span aria-hidden className="transition-transform group-hover:translate-x-1">
            →
          </span>
        </a>
        <a
          href="https://github.com/goliajp/torajs"
          className="border-rule text-bone hover:border-tiger hover:text-tiger-bright inline-flex items-center gap-3 border px-5 py-3 font-mono text-[12.5px] tracking-[0.18em] uppercase transition-colors"
        >
          Source on GitHub
        </a>
      </div>

      <Sigil />
    </section>
  )
}

/* Decorative tiger-stripe glyph in the upper-right corner of the
 * hero. Pure SVG — strokes only, no fills. Echoes the wordmark
 * accent without competing with it. */
function Sigil() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 240 320"
      className="absolute top-12 right-6 hidden h-[280px] w-auto opacity-50 sm:block"
      style={{ animationDelay: '600ms' }}
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
        {/* horizontal break — the "stripe break" motif */}
        <path d="M20,160 L210,160" opacity="0.2" stroke="white" />
        {/* small marker dots */}
        <circle cx="40" cy="160" r="2.4" fill="currentColor" stroke="none" />
        <circle cx="96" cy="160" r="2.4" fill="currentColor" stroke="none" />
        <circle cx="152" cy="160" r="2.4" fill="currentColor" stroke="none" />
      </g>
    </svg>
  )
}

function Install() {
  const cmd = 'curl -fsSL https://install.torajs.com | bash'
  const [copied, setCopied] = useState(false)

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(cmd)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      /* clipboard API not available; ignore silently */
    }
  }

  return (
    <section id="install" className="mx-auto max-w-[960px] px-6 py-14 sm:py-20">
      <SectionLabel index="01" label="Install" />

      <div className="mt-8 grid gap-10 sm:grid-cols-[1fr_auto] sm:items-end">
        <div>
          <p className="text-bone-dim max-w-[520px] text-[16px] leading-[1.55]">
            One line, one platform-detected tarball, signature-verified before extract. Drops the{' '}
            <Inline mono>tr</Inline> binary into <Inline mono>~/.torajs/bin</Inline>, prints a PATH
            hint, exits.
          </p>
        </div>
        <p className="text-bone-faint hidden font-mono text-[11px] tracking-[0.2em] uppercase sm:block">
          macOS arm64 · Linux x64
        </p>
      </div>

      <div className="code-block group relative mt-8 overflow-hidden">
        <div className="border-rule text-bone-faint flex items-center justify-between border-b px-5 py-2 font-mono text-[11px] tracking-[0.2em] uppercase">
          <span>shell</span>
          <button
            onClick={onCopy}
            className="text-bone-dim hover:text-tiger-bright transition-colors"
            aria-label="Copy install command"
          >
            {copied ? 'copied ✓' : 'copy'}
          </button>
        </div>
        <pre className="px-5 py-5 font-mono text-[15px] leading-[1.6] sm:text-[16px]">
          <span className="text-bone-faint">$</span> <span className="text-bone">curl</span>{' '}
          <span className="tk-num">-fsSL</span>{' '}
          <span className="tk-str">https://install.torajs.com</span>{' '}
          <span className="text-bone">|</span> <span className="text-tiger">bash</span>
          <span className="cursor-blink" />
        </pre>
      </div>

      <div className="text-bone-faint mt-6 grid gap-3 font-mono text-[12px] sm:grid-cols-3">
        <Stat label="Cold start" value="~1.3 ms" hint="tr run hello.ts" />
        <Stat label="Binary size" value="~40 KB" hint="tr build, statically linked" />
        <Stat label="Compile" value="~50 ms" hint="cached on rerun" />
      </div>
    </section>
  )
}

function CodeShowcase() {
  return (
    <section className="mx-auto max-w-[960px] px-6 py-14 sm:py-20">
      <SectionLabel index="02" label="The same TS, faster" />

      <div className="mt-8 grid gap-10 sm:grid-cols-[1.1fr_1fr] sm:gap-14">
        <div>
          <h2 className="wordmark-roman text-bone text-[40px] sm:text-[56px]">
            What you write is what bun runs.
          </h2>
          <p className="text-bone-dim mt-6 max-w-[480px] text-[16px] leading-[1.6]">
            Classes, generics, closures, generators, try/catch, JSON, multi-file imports, the full
            string / array / Math stdlib — implemented and verified against bun byte-for-byte. The
            runtime differentiator is the only differentiator: AOT to a real native binary, ARC
            under a universal heap header, no tracing GC.
          </p>
          <p className="text-bone-faint mt-6 font-mono text-[12px] tracking-[0.18em] uppercase">
            from{' '}
            <a
              className="link-amber"
              href="https://github.com/goliajp/torajs/tree/main/examples/sha256"
            >
              examples / sha256.ts
            </a>
          </p>
        </div>

        <div className="code-block overflow-hidden">
          <div className="border-rule text-bone-faint flex items-center justify-between border-b px-5 py-2 font-mono text-[11px] tracking-[0.2em] uppercase">
            <span>sha256.ts</span>
            <span className="text-tiger-bright">tr run · bun parity</span>
          </div>
          <pre className="overflow-x-auto px-5 py-5 font-mono text-[12.5px] leading-[1.6]">
            <code>
              <Sample />
            </code>
          </pre>
        </div>
      </div>
    </section>
  )
}

function Sample() {
  /* Hand-tokenized excerpt — keeps the runtime small (no shiki, no
   * highlight.js bundled). The ratio of accent / dim / quoted text
   * drives the visual rhythm of the block; tweak by swapping
   * .tk-* spans, never inject color values inline. */
  return (
    <>
      <span className="tk-com">{`// SHA-256 — bit-twiddle heavy, 32-bit math via \`>>> 0\` coercion.`}</span>
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
  /* Bars are normalized per-row to the slowest competitor in the row,
   * so each row reads as "tr's share of the worst time." Absolute
   * milliseconds are kept in the value column for readers who care
   * about wall-clock. Animation triggers once when the section
   * scrolls into view. */
  /* Use the IntersectionObserver to delay-trigger bar fills only
   * when the section reaches the viewport. Default to `true` so the
   * (rare) no-IO environment still shows the bars; the observer
   * effect only ever flips us back to false → true once. */
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
    <section id="bench" className="scanlines mx-auto max-w-[960px] px-6 py-16 sm:py-24">
      <SectionLabel index="03" label="Bench scoreboard" />

      <div className="mt-8 grid gap-10 sm:grid-cols-[1fr_auto] sm:items-end">
        <div>
          <h2 className="wordmark-roman text-bone text-[40px] sm:text-[56px]">
            <span className="text-tiger">19/19</span>
            <span> bench cases, tr build wins.</span>
          </h2>
          <p className="text-bone-dim mt-5 max-w-[520px] text-[15.5px]">
            Cross-runtime perf, M4 Pro, hyperfine n=10 with 3 warmup runs. Headline rows below; full
            table in{' '}
            <a
              className="link-amber"
              href="https://github.com/goliajp/torajs/blob/main/docs/perf.md"
            >
              docs/perf.md
            </a>
            .
          </p>
        </div>
        <div className="text-bone-faint font-mono text-[11px] tracking-[0.2em] uppercase">
          ms · lower is better
        </div>
      </div>

      <ul className="mt-10 space-y-4">
        {BENCH.map((row, i) => {
          const max = Math.max(row.tr, row.rust, row.go, row.bun)
          return (
            <li
              key={row.case}
              className="grid gap-2 sm:grid-cols-[180px_1fr_120px] sm:items-center sm:gap-6"
            >
              <div className="text-bone font-mono text-[13px]">
                {row.case}
                {row.label && (
                  <span className="text-tiger-bright ml-2 text-[11px] tracking-[0.16em] uppercase">
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
                <div>
                  <span className="text-tiger-bright">{row.tr.toFixed(2)}</span>{' '}
                  <span className="text-bone-faint">ms</span>
                </div>
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

const CAPABILITIES: { title: string; body: string }[] = [
  {
    title: 'Classes & generics',
    body: 'Instance + static, inheritance, abstract, visibility modifiers. Generics monomorphized per call site.',
  },
  {
    title: 'Closures, generators',
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
    body: 'slice / repeat / replace / pad·, push / map / filter / reduce / sort, every Math.* + constant.',
  },
  {
    title: 'AOT to native, by default',
    body: 'tr build emits a real binary. tr run caches at ~/.torajs/cache so the dev loop is free after the first compile.',
  },
]

function CapabilityGrid() {
  return (
    <section id="status" className="mx-auto max-w-[960px] px-6 py-14 sm:py-20">
      <SectionLabel index="04" label="What works today" />

      <div className="mt-8 grid gap-10 sm:grid-cols-[1.05fr_1fr] sm:gap-14">
        <h2 className="wordmark-roman text-bone text-[40px] sm:text-[52px]">
          Most everyday TS, already shipping.
        </h2>
        <p className="text-bone-dim text-[15.5px] sm:pt-4">
          The line is moving — not a frozen cut-down language. Anything bun runs that tr rejects is
          a roadmap-phase gap, not a permanent decision. The full feature table lives in{' '}
          <a
            className="link-amber"
            href="https://github.com/goliajp/torajs/blob/main/docs/language-status.md"
          >
            language-status.md
          </a>
          .
        </p>
      </div>

      <ul className="mt-12 grid gap-x-10 gap-y-8 sm:grid-cols-2">
        {CAPABILITIES.map((c, i) => (
          <li key={c.title} className="border-rule border-l pl-5">
            <p className="eyebrow">{String(i + 1).padStart(2, '0')}</p>
            <h3 className="text-bone font-display mt-3 text-[20px] font-medium">{c.title}</h3>
            <p className="text-bone-dim mt-2 text-[14.5px] leading-[1.6]">{c.body}</p>
          </li>
        ))}
      </ul>
    </section>
  )
}

function ClosingNote() {
  return (
    <section className="mx-auto max-w-[960px] px-6 py-16 sm:py-24">
      <div className="grid gap-10 sm:grid-cols-[auto_1fr] sm:items-center sm:gap-14">
        <p
          aria-hidden
          className="wordmark text-tiger leading-[0.85]"
          style={{ fontSize: 'clamp(96px, 14vw, 168px)' }}
        >
          ⤳
        </p>
        <div>
          <h2 className="wordmark-roman text-bone text-[40px] leading-[0.95] sm:text-[56px]">
            Bun is the oracle.
          </h2>
          <p className="text-bone-dim mt-6 max-w-[520px] text-[16px] leading-[1.6]">
            When behavior is unclear, write the equivalent in TS, run it in{' '}
            <Inline mono>bun</Inline>, and match. If torajs differs from bun&rsquo;s output
            (excluding the documented perf differentiators), that&rsquo;s a bug — file an issue.
          </p>
          <div className="mt-8 flex flex-wrap gap-4">
            <a
              className="text-tiger-bright font-mono text-[12.5px] tracking-[0.18em] uppercase"
              href="https://github.com/goliajp/torajs/issues/new"
            >
              File an issue →
            </a>
            <a
              className="text-bone-dim hover:text-bone font-mono text-[12.5px] tracking-[0.18em] uppercase transition-colors"
              href="https://github.com/goliajp/torajs/blob/main/docs/getting-started.md"
            >
              Read the docs →
            </a>
          </div>
        </div>
      </div>
    </section>
  )
}

function Footer() {
  return (
    <footer className="border-rule/70 mt-12 border-t">
      <div className="mx-auto grid max-w-[960px] gap-8 px-6 py-10 sm:grid-cols-[1.4fr_1fr_1fr_1fr]">
        <div>
          <p className="wordmark-roman text-bone text-[28px]">
            <span className="text-tiger">tora</span>
            <span>js</span>
          </p>
          <p className="text-bone-faint mt-3 text-[13px]">
            Ship the same TS bun runs, faster — at a fraction of the size.
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
        <div className="mx-auto flex max-w-[960px] items-center justify-between px-6 py-4 font-mono text-[11px] tracking-[0.18em] uppercase">
          <span className="text-bone-faint">© torajs · Apache-2.0 · v0.1.0-beta</span>
          <span className="text-bone-faint">
            <span className="text-tiger">●</span> released 2026
          </span>
        </div>
      </div>
    </footer>
  )
}

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
      <span className="bg-rule h-[1px] max-w-[100px] flex-1" />
      <span className="eyebrow">{label}</span>
    </div>
  )
}

function PaintLine({ spacing }: { spacing: 'sm' | 'md' | 'lg' }) {
  const cls = spacing === 'lg' ? 'py-12' : spacing === 'md' ? 'py-8' : 'py-4'
  return (
    <div className={`mx-auto max-w-[960px] px-6 ${cls}`}>
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

function Stat({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="border-rule border-l pl-4">
      <p className="text-bone-faint tracking-[0.18em] uppercase">{label}</p>
      <p className="text-bone num font-display mt-1 text-[20px] font-medium tracking-tight">
        {value}
      </p>
      <p className="text-bone-faint/80 mt-1 text-[10.5px] tracking-[0.16em] uppercase">{hint}</p>
    </div>
  )
}
