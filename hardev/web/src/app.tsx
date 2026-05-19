import rawData from './data.json'
import type { Pillar, SnapshotData } from './types'

const data = rawData as unknown as SnapshotData

const PILLAR_TITLES: Record<string, string> = {
  devperf: 'Dev-loop performance',
  cleanup: 'Garbage / stale-artifact control',
  taskq: 'L1–L4 plan governance',
  bench: 'Benchmark · coverage · verdict',
}

function fmt(n: number | null): string {
  if (n == null) return '—'
  return n.toFixed(2)
}

function todayLabel(): string {
  const d = new Date(data.generatedAt)
  const y = d.getUTCFullYear()
  const m = String(d.getUTCMonth() + 1).padStart(2, '0')
  const day = String(d.getUTCDate()).padStart(2, '0')
  return `${y} · ${m} · ${day}`
}

// take the leading fact from a metrics.md `now` cell, drop the [M]/✅ noise
function shortStatus(p: Pillar): string {
  let s = p.status
    .replace(/\[[MAD]\]/g, '')
    .replace(/✅/g, '')
    .replace(/\s+/g, ' ')
    .trim()
  // first clause up to an em-dash or sentence stop, capped
  const cut = s.search(/ — | – | \(/)
  if (cut > 24) s = s.slice(0, cut).trim()
  if (s.length > 130) s = s.slice(0, 127).trimEnd() + '…'
  return s
}

function Header() {
  return (
    <header>
      <div className="container">
        <span className="brand">
          hardev
          <span className="dot" />
        </span>
        <div className="right">
          <span>v{data.hardevVersion}</span>
          <span>{todayLabel()}</span>
          <span className="live">torajs R&amp;D-support · live</span>
        </div>
      </div>
    </header>
  )
}

function Hero() {
  return (
    <div className="hero">
      <h1>
        torajs dev,
        <br />
        <span className="accent">instrumented.</span>
      </h1>
      <p className="lede">
        <b>hardev</b> is the Rust-specialized R&amp;D-support framework incubating inside torajs —
        the company&rsquo;s bun-class AOT TypeScript runtime. Four pillars (devperf · cleanup ·
        taskq · bench), metrics-first, acceptance-gated. It makes the develop → verify → ship loop
        fast, clean, and trustworthy <em>without ever trading away verification coverage</em>.
      </p>

      <div className="kpi">
        {data.headline.map((h) => (
          <div className="kpi-item" key={h.label}>
            <div className="kpi-num">
              {h.value}
              <span className="x">{h.unit}</span>
            </div>
            <div className="kpi-label">{h.label}</div>
            <div className="kpi-sub">{h.sub}</div>
          </div>
        ))}
      </div>
    </div>
  )
}

function Pillars() {
  return (
    <section>
      <div className="h2">
        <span className="num">01</span>The four pillars
        <span className="sub">metrics-first · acceptance-gated</span>
      </div>
      <div className="pillars">
        {data.pillars.map((p) => (
          <div className="pillar" key={p.key}>
            <div className="pillar-tag">{p.name}</div>
            <h3>{PILLAR_TITLES[p.key] ?? p.name}</h3>
            <p className="scope">{p.scope}</p>
            <div className="metric">
              <span className="mk">{p.metricName ? p.metricName : 'current status'}</span>
              {shortStatus(p)}
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}

function Bench() {
  const b = data.bench
  return (
    <section>
      <div className="h2">
        <span className="num">02</span>
        {b.caseCount} benchmarks · {b.wonCount} wins
        <span className="sub">vs bun-aot, bun-jsc, node-v8</span>
      </div>
      <div className="bench-summary">
        <span>
          <b>{fmt(b.geomeanBunAot)}×</b> geomean vs bun-aot
        </span>
        <span>
          <b>{fmt(b.geomeanNodeV8)}×</b> geomean vs node-v8
        </span>
        <span>
          <b>{fmt(b.peak)}×</b> peak speedup ({b.peakCase})
        </span>
        <span>
          <b>{fmt(b.min)}×</b> minimum speedup ({b.minCase})
        </span>
      </div>
      <table className="bench">
        <thead>
          <tr>
            <th>benchmark</th>
            <th>bun-aot</th>
            <th>bun-jsc</th>
            <th>node-v8</th>
            <th>torajs</th>
            <th>× vs bun</th>
          </tr>
        </thead>
        <tbody>
          {b.rows.map((r) => {
            const lo = (r.speedup ?? 0) < 2
            return (
              <tr key={r.case}>
                <td className="case">{r.case}</td>
                <td>{fmt(r.bunAot)}</td>
                <td>{fmt(r.bunJsc)}</td>
                <td>{fmt(r.nodeV8)}</td>
                <td className="tora">{fmt(r.torajs)}</td>
                <td className={lo ? 'x lo' : 'x'}>{fmt(r.speedup)}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
      <p className="bench-foot">
        all values are run_ms · hyperfine · darwin · arm64 · {b.host} · all benchmarks run solo ·
        data: {b.sourceFile} @ {b.gitSha.slice(0, 7)} · started {b.startedAt}
      </p>
    </section>
  )
}

function Progress() {
  const cl = data.changelog
  return (
    <section>
      <div className="h2">
        <span className="num">03</span>Progress
        <span className="sub">hardev changelog · newest first</span>
      </div>

      <div className="status-grid">
        <div className="stat-cell">
          <div className="stat-v">
            {data.conformance.pass}
            <span className="d">
              /{data.conformance.fail}/{data.conformance.skip}
            </span>
          </div>
          <div className="stat-l">conformance · 0 fail</div>
        </div>
        <div className="stat-cell">
          <div className="stat-v">v{data.hardevVersion}</div>
          <div className="stat-l">hardev version</div>
        </div>
        <div className="stat-cell">
          <div className="stat-v">
            4<span className="u">/ 4</span>
          </div>
          <div className="stat-l">pillars shipped</div>
        </div>
        <div className="stat-cell">
          <div className="stat-v">
            {data.bench.caseCount}
            <span className="d">/{data.bench.wonCount}</span>
          </div>
          <div className="stat-l">benchmarks won</div>
        </div>
      </div>

      <div className="timeline">
        {cl.map((r) => (
          <div className="tl-row" key={r.version}>
            <div className="ph">v{r.version}</div>
            <div className="date">{r.date}</div>
            <div className="body">
              <div className="title">{r.title}</div>
              {r.bullets[0] && <div className="sum">{r.bullets[0]}</div>}
            </div>
          </div>
        ))}
      </div>

      <div className="h2" style={{ marginTop: 40 }}>
        <span className="num">03b</span>Recent commits
        <span className="sub">git log · torajs HEAD</span>
      </div>
      <div className="commits">
        {data.commits.map((c) => (
          <div className="cm-row" key={c.hash}>
            <span className="hash">{c.hash}</span>
            <span className="subj">{c.subject}</span>
          </div>
        ))}
      </div>
    </section>
  )
}

function Footer() {
  return (
    <footer>
      <div className="container">
        <span className="quote">
          &ldquo;Measure before you optimize. Never trade away what is verified.&rdquo;
        </span>
        <span>
          commit <code style={{ color: 'var(--ink)' }}>{data.headSha}</code>
        </span>
        <span>Rust · LLVM · static C runtime</span>
        <span>
          <a href="mailto:takagi@golia.jp">takagi@golia.jp</a>
        </span>
      </div>
    </footer>
  )
}

export function App() {
  return (
    <>
      <Header />
      <div className="container">
        <Hero />
        <Pillars />
        <Bench />
        <Progress />
      </div>
      <Footer />
    </>
  )
}
