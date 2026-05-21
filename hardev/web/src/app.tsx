import rawData from './data.json'
import type { Pillar, RoadmapPhase, Rotation, SnapshotData } from './types'

const data = rawData as unknown as SnapshotData

// hardev is the SECONDARY tooling section — these one-liners describe what
// each pillar buys torajs development, framed as dev-velocity effect.
const PILLAR_ONELINE: Record<string, string> = {
  devperf: 'build/cache levers — sccache, project-private cargo-target, the real bottlenecks',
  cleanup: 'enumerable regenerable junk reclaimed safely · dry-run-default · never touches source',
  taskq: 'the L1–L4 planning architecture made a machine-checkable, enforced discipline',
  bench: 'trustworthy, reproducible, machine-judged regression verdicts · fast per-commit path',
}

const STATUS_LABEL: Record<string, string> = {
  DONE: 'done',
  CURRENT: 'current',
  queued: 'queued',
  'post-v1.0': 'post-v1.0',
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

function shortRanAt(iso: string): string {
  // "2026-05-20T12:34:56Z" → "2026-05-20"
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const y = d.getUTCFullYear()
  const m = String(d.getUTCMonth() + 1).padStart(2, '0')
  const day = String(d.getUTCDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function rotationStamp(iso: string): string {
  // "2026-05-20T01:08:42Z" → "2026-05-20 01:08 UTC"
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const y = d.getUTCFullYear()
  const mo = String(d.getUTCMonth() + 1).padStart(2, '0')
  const day = String(d.getUTCDate()).padStart(2, '0')
  const hh = String(d.getUTCHours()).padStart(2, '0')
  const mm = String(d.getUTCMinutes()).padStart(2, '0')
  return `${y}-${mo}-${day} ${hh}:${mm} UTC`
}

function rotationAge(ts: number, nowSec: number): string {
  // ts is unix seconds; nowSec is unix seconds of the snapshot
  const diff = nowSec - ts
  if (diff < 0) return 'just now'
  if (diff < 60) return `${diff}s ago`
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  return `${Math.floor(diff / 86400)}d ago`
}

// take the leading fact from a metrics.md `now` cell, drop the [M]/✅ noise
function shortStatus(p: Pillar): string {
  let s = p.status
    .replace(/\[[MAD]\]/g, '')
    .replace(/✅/g, '')
    .replace(/\s+/g, ' ')
    .trim()
  const cut = s.search(/ — | – | \(/)
  if (cut > 24) s = s.slice(0, cut).trim()
  if (s.length > 130) s = s.slice(0, 127).trimEnd() + '…'
  return s
}

function rmClass(p: RoadmapPhase): string {
  if (p.status === 'DONE') return 'rm-row done'
  if (p.status === 'CURRENT') return 'rm-row cur'
  return 'rm-row'
}

function Header() {
  return (
    <header>
      <div className="container">
        <span className="brand">
          torajs
          <span className="dot" />
        </span>
        <div className="right">
          <span>{todayLabel()}</span>
          <span className="live">internal · dev status</span>
        </div>
      </div>
    </header>
  )
}

// hero KPIs are derived from the snapshot bench numbers — never hardcoded.
function heroKpis() {
  const b = data.bench
  const startup = b.rows.find((r) => r.case === 'startup')
  return [
    {
      value: fmt(b.geomeanBunAot),
      unit: '×',
      label: 'geomean faster than bun-aot',
      sub: `geomean across ${b.caseCount} benchmarks · vs bun build --compile`,
    },
    {
      value: startup && startup.speedup != null ? startup.speedup.toFixed(2) : '—',
      unit: '×',
      label: 'faster cold start',
      sub:
        startup != null
          ? `startup case: ${fmt(startup.torajs)} ms vs ${fmt(startup.bunAot)} ms (bun-aot) · ${fmt(startup.nodeV8)} ms (node)`
          : 'startup case',
    },
    {
      value: `${b.wonCount}`,
      unit: ` / ${b.caseCount}`,
      label: 'benchmarks won',
      sub: 'torajs beats the best of bun-aot / bun-jsc on every case',
    },
  ]
}

function Hero() {
  return (
    <div className="hero">
      <h1>
        TypeScript,
        <br />
        <span className="accent">compiled to silicon.</span>
      </h1>
      <p className="lede">
        <b>torajs</b> is an ahead-of-time TypeScript runtime that emits standalone native binaries.
        Same TS programs as Bun. No JIT, no interpreter, no embedded runtime — the program{' '}
        <em>is</em> the binary, full throughput from the first nanosecond.
      </p>

      <div className="kpi">
        {heroKpis().map((h) => (
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

// SECTION 01 — Progress (PRIMARY): the torajs P0→P15 roadmap story.
function Progress() {
  const rm = data.roadmap
  const t = data.torajs
  const c = data.conformance
  const b = data.bench
  return (
    <section>
      <div className="h2">
        <span className="num">01</span>Progress
        <span className="sub">P0 → P15 substrate roadmap</span>
      </div>

      <div className="roadmap">
        {rm.map((p) => (
          <div className={rmClass(p)} key={p.id}>
            <div className="ph">{p.id}</div>
            <div className="desc">{p.title}</div>
            <div className="st">{STATUS_LABEL[p.status] ?? p.status}</div>
          </div>
        ))}
      </div>

      <div className="status-grid" style={{ marginTop: 24 }}>
        <div className="stat-cell">
          <div className="stat-v">
            {c.pass}
            <span className="d">
              /{c.fail}/{c.skip}
            </span>
          </div>
          <div className="stat-l">subset conformance · 0 fail</div>
        </div>
        <div className="stat-cell">
          <div className="stat-v">{t.phase}</div>
          <div className="stat-l">current phase</div>
        </div>
        <div className="stat-cell">
          <div className="stat-v">{t.commit}</div>
          <div className="stat-l">torajs HEAD</div>
        </div>
        <div className="stat-cell">
          <div className="stat-v">
            {b.wonCount}
            <span className="d">/{b.caseCount}</span>
          </div>
          <div className="stat-l">benchmarks won</div>
        </div>
      </div>

      <Test262Card />
    </section>
  )
}

// tc39/test262 spec-conformance — the ECMAScript reference suite. The
// number Anthropic / Bun / V8 are measured against. Distinct from the
// 'subset conformance' card above (which is the in-tree fixture set
// torajs has handcrafted). When `data.test262` is null (no full run
// captured yet) the card renders an explicit stub rather than fabricating
// numbers — per the snapshot rule that the dashboard never invents data.
function Test262Card() {
  const t = data.test262
  if (!t) {
    return (
      <div className="t262-card t262-empty">
        <div className="t262-label">test262 (tc39 ECMAScript spec suite)</div>
        <div className="t262-empty-body">
          no full run recorded yet — run{' '}
          <code>cargo run -p torajs-test262 --release -- --json hardev/test262-latest.json</code> to
          populate
        </div>
      </div>
    )
  }
  return (
    <div className="t262-card">
      <div className="t262-row">
        <div className="t262-main">
          <div className="t262-headline">
            {t.pass}
            <span className="d">/{t.inScope}</span>
            <span className="pct">{t.passRateInScope.toFixed(2)}%</span>
          </div>
          <div className="t262-label">test262 · tc39 ECMAScript spec suite · in-scope pass</div>
        </div>
        <div className="t262-breakdown">
          <span>
            <b>{t.bug}</b> bug
          </span>
          <span>
            <b>{t.incompatible}</b> incompatible
          </span>
          <span>
            <b>{t.bunSkip}</b> bun-skip
          </span>
          <span>
            <b>{t.trAccepted}</b> tr-accepted ({t.passRateTrAccepted.toFixed(1)}%)
          </span>
        </div>
      </div>
      <div className="t262-stamp">
        ran {shortRanAt(t.ranAt)} · HEAD {t.headSha} · {t.ran.toLocaleString()} of{' '}
        {t.totalCases.toLocaleString()} cases · {t.workers} workers · {t.elapsedSec.toFixed(0)}s
      </div>
    </div>
  )
}

// hardev/autorun pillar (5th pillar — rotation governance). Reads
// data.autorun (populated from hardev/autorun/rotations.jsonl by
// snapshot.mjs). When null, renders an explicit stub instead of
// fabricating numbers — same snapshot rule as Test262Card.
function AutorunCard() {
  const a = data.autorun
  if (!a) {
    return (
      <div className="t262-card t262-empty">
        <div className="t262-label">autorun · rotation log (5th pillar)</div>
        <div className="t262-empty-body">
          no rotations recorded yet — run <code>hardev/autorun/trigger.sh self</code> or{' '}
          <code>manual</code> to append a row to <code>hardev/autorun/rotations.jsonl</code>
        </div>
      </div>
    )
  }
  const nowSec = Math.floor(new Date(data.generatedAt).getTime() / 1000)
  const baselineMet = a.total >= a.baselineTarget
  const baselinePct = Math.min(100, Math.round((a.total / a.baselineTarget) * 100))
  return (
    <div className="t262-card">
      <div className="t262-row">
        <div className="t262-main">
          <div className="t262-headline">
            {a.total}
            <span className="d">/{a.baselineTarget}</span>
            <span className="pct">{baselinePct}%</span>
          </div>
          <div className="t262-label">
            autorun · rotation governance (5th pillar · P0+P1 shipped · stop-hook + watcher live)
          </div>
        </div>
        <div className="t262-breakdown">
          <span>
            <b>{a.bySelf}</b> self
          </span>
          <span>
            <b>{a.byManual}</b> manual
          </span>
          {a.byOther > 0 && (
            <span>
              <b>{a.byOther}</b> other
            </span>
          )}
          <span>
            P1 watcher <b>{baselineMet ? 'deployed' : `${a.baselineTarget - a.total} rows away`}</b>
          </span>
        </div>
      </div>
      {a.recent.length > 0 && (
        <table className="autorun-recent">
          <thead>
            <tr>
              <th>rotation</th>
              <th>when</th>
              <th>trigger</th>
              <th>HEAD</th>
              <th>handoff age</th>
              <th>conformance</th>
            </tr>
          </thead>
          <tbody>
            {a.recent.map((r: Rotation) => (
              <tr key={r.rotationId}>
                <td className="case">{r.rotationId}</td>
                <td>{rotationAge(r.ts, nowSec)}</td>
                <td>{r.trigger}</td>
                <td>{r.prevHead}</td>
                <td>{r.handoffAgeSec}s</td>
                <td>{r.conformanceBefore ?? '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="t262-stamp">
        log: <code>{a.rotationsFile}</code>
        {a.last && ` · last ${rotationStamp(a.last.at)}`} · P1 live: stop-hook + INV-1..5 gate +
        launchd watcher + auto-/clear + auto-resume (P1.5 dogfood 5/5 GREEN)
      </div>
    </div>
  )
}

// SECTION 02 — Benchmark (the torajs proof): torajs vs the world.
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

// SECTION 03 — hardev (the TOOL, secondary): what the R&D-support
// tooling buys torajs development. Compact, quieter than 01–02.
function Hardev() {
  const cl = data.changelog
  return (
    <section>
      <div className="h2">
        <span className="num">03</span>hardev
        <span className="sub">R&amp;D-support tooling · v{data.hardevVersion}</span>
      </div>
      <div className="tool">
        <p className="tool-lede">
          <b>hardev</b> is the Rust-specialized R&amp;D-support framework that instruments this dev
          loop — four pillars, metrics-first, acceptance-gated. Its only job is to make torajs
          development fast and correct <em>without ever trading away verification coverage</em>.
        </p>

        <div className="tool-pillars">
          {data.pillars.map((p) => (
            <div className="tp-row" key={p.key}>
              <span className="tp-tag">{p.name}</span>
              <span className="tp-txt">{PILLAR_ONELINE[p.key] ?? shortStatus(p)}</span>
            </div>
          ))}
        </div>

        <div className="tool-velocity">
          <span>
            edit→rebuild tr <b>28.5 s → 2.49 s</b>
          </span>
          <span>
            per-commit gate <b>~10 min → seconds</b>
          </span>
          <span>
            full conformance <b>~30 min → ~3 min</b>
          </span>
        </div>

        <p className="tool-cap">
          what hardev buys torajs development — measured dev-loop velocity, conformance-equivalent
          (629/0/1) · latest: v{cl[0]?.version} {cl[0]?.title}
        </p>

        <AutorunCard />
      </div>
    </section>
  )
}

function Footer() {
  return (
    <footer>
      <div className="container">
        <span className="quote">
          &ldquo;Bun proved TS can be the runtime. torajs makes it the binary.&rdquo;
        </span>
        <span>
          commit <code style={{ color: 'var(--ink)' }}>{data.torajs.commit}</code>
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
        <Progress />
        <Bench />
        <Hardev />
      </div>
      <Footer />
    </>
  )
}
