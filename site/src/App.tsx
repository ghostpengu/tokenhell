import { useEffect, useState, type CSSProperties, type ReactNode } from 'react'
import AnimatedContent from './components/rb/AnimatedContent'
import BlurText from './components/rb/BlurText'
import CountUp from './components/rb/CountUp'
import LiquidEther from './components/rb/LiquidEther'
import LogoLoop from './components/rb/LogoLoop'
import Magnet from './components/rb/Magnet'
import RotatingText from './components/rb/RotatingText'
import SpotlightCard from './components/rb/SpotlightCard'
import { Flame } from './components/Flame'
import Panel, { fmtTokens, useLiveTokens } from './components/Panel'
import appIcon from './assets/app-icon.png'

const REPO = 'https://github.com/ghostpengu/tokenhell'
const DOWNLOAD = `${REPO}/releases/latest`

const TOOLS = [
  { name: 'Claude Code', color: 'var(--color-claude)' },
  { name: 'Codex', color: 'var(--color-codex)' },
  { name: 'Cursor', color: 'var(--color-cursor)' },
  { name: 'Grok', color: 'var(--color-grok)' },
  { name: 'Copilot', color: 'var(--color-copilot)' },
  { name: 'Gemini CLI', color: 'var(--color-gemini)' },
  { name: 'OpenCode', color: 'var(--color-opencode)' },
  { name: 'Qwen', color: 'rgb(255 246 240 / 0.42)' },
  { name: 'Kimi', color: 'rgb(255 246 240 / 0.42)' },
  { name: 'Pi', color: 'var(--color-pi)' },
]

/* ---------- small pieces ---------- */

function GitHubMark({ className = '' }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" className={className} fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  )
}

function AppleMark({ className = '' }: { className?: string }) {
  return (
    <svg viewBox="0 0 17 20" className={className} fill="currentColor" aria-hidden>
      <path d="M14.06 10.62c-.02-2.26 1.85-3.35 1.93-3.4-1.05-1.54-2.69-1.75-3.27-1.77-1.39-.14-2.72.82-3.42.82-.71 0-1.8-.8-2.95-.78-1.52.02-2.92.88-3.7 2.24-1.58 2.74-.4 6.79 1.13 9.01.75 1.09 1.65 2.31 2.82 2.26 1.13-.04 1.56-.73 2.93-.73 1.36 0 1.75.73 2.95.71 1.22-.02 1.99-1.1 2.73-2.2.86-1.26 1.22-2.49 1.24-2.55-.03-.01-2.37-.91-2.39-3.61zM11.82 3.98c.62-.76 1.04-1.8.93-2.85-.9.04-1.99.6-2.63 1.35-.58.67-1.08 1.74-.95 2.76 1 .08 2.03-.51 2.65-1.26z" />
    </svg>
  )
}

function Copy({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [done, setDone] = useState(false)
  return (
    <button
      onClick={() => {
        navigator.clipboard?.writeText(text).then(() => {
          setDone(true)
          setTimeout(() => setDone(false), 1400)
        })
      }}
      className="rounded-md px-2 py-1 text-[11px] text-ink-3 transition-colors hover:bg-fill-2 hover:text-ink-1"
    >
      {done ? 'Copied' : label}
    </button>
  )
}

function Code({ children, copy }: { children: string; copy?: string }) {
  return (
    <div className="group relative rounded-2xl border-[0.5px] border-hair bg-black/40">
      <div className="absolute top-2 right-2">
        <Copy text={copy ?? children.replace(/^\$ /gm, '')} />
      </div>
      <pre className="overflow-x-auto p-4 pr-16 font-mono text-[13px] leading-relaxed text-ink-2">
        {children.split('\n').map((line, i) => (
          <div key={i}>
            {line.startsWith('$ ') ? (
              <>
                <span className="text-flame select-none">$ </span>
                <span className="text-ink-1">{line.slice(2)}</span>
              </>
            ) : (
              line
            )}
          </div>
        ))}
      </pre>
    </div>
  )
}

function Reveal({ children, delay = 0, className }: { children: ReactNode; delay?: number; className?: string }) {
  return (
    <AnimatedContent distance={40} duration={0.9} ease="power3.out" blur={6} delay={delay} threshold={0.12} className={className}>
      {children}
    </AnimatedContent>
  )
}

function SectionHead({ kicker, title, children }: { kicker: string; title: string; children?: ReactNode }) {
  return (
    <Reveal className="mx-auto max-w-2xl text-center">
      <div className="text-[13px] font-medium text-flame">{kicker}</div>
      <h2 className="mt-3 text-[clamp(30px,4.6vw,48px)] leading-[1.05] font-semibold tracking-[-0.035em] text-balance">{title}</h2>
      {children && <p className="mt-4 text-[17px] leading-relaxed text-pretty text-ink-2">{children}</p>}
    </Reveal>
  )
}

const card: CSSProperties = {
  ['--spotlight-card-surface' as string]: '#141013',
  ['--spotlight-card-border' as string]: 'rgba(255,255,255,0.07)',
}

function Card({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <SpotlightCard
      className={`!p-6 sm:!p-7 ${className}`}
      spotlightColor="rgba(255, 122, 58, 0.22)"
      spotlightSize={420}
      style={card}
    >
      {children}
    </SpotlightCard>
  )
}

function CardTitle({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div>
      <h3 className="text-[19px] font-semibold tracking-tight">{title}</h3>
      <p className="mt-2 text-[14.5px] leading-relaxed text-pretty text-ink-2">{children}</p>
    </div>
  )
}

function Gauge({ label, pct, delay = 0 }: { label: string; pct: number; delay?: number }) {
  const c = pct >= 85 ? 'var(--color-hot)' : pct >= 50 ? 'var(--color-warn)' : 'var(--color-good)'
  return (
    <div className="grid grid-cols-[92px_1fr_40px] items-center gap-3 text-[13px]">
      <span className="text-ink-2">{label}</span>
      <span className="h-[6px] overflow-hidden rounded-full bg-fill-2">
        <span className="bar-grow block h-full rounded-full" style={{ width: `${pct}%`, background: c, animationDelay: `${delay}ms` }} />
      </span>
      <span className="tnum text-right font-semibold">{pct}%</span>
    </div>
  )
}

/* ---------- sections ---------- */

function MenuBar({ live }: { live: number }) {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 15_000)
    return () => clearInterval(id)
  }, [])
  const clock = now.toLocaleString('en-US', { weekday: 'short', hour: 'numeric', minute: '2-digit' })

  return (
    <header className="menubar fixed inset-x-0 top-0 z-50">
      <nav className="mx-auto flex h-11 max-w-6xl items-center gap-1 px-4 text-[13.5px]">
        <a href="#top" className="mr-3 flex items-center gap-2 font-semibold">
          <img src={appIcon} alt="" className="h-6 w-6" />
          TokenHell
        </a>
        {[
          ['Features', '#features'],
          ['Sources', '#sources'],
          ['Privacy', '#privacy'],
          ['Install', '#install'],
        ].map(([l, h]) => (
          <a key={h} href={h} className="hidden rounded-md px-2.5 py-1 text-ink-2 transition-colors hover:bg-fill-2 hover:text-ink-1 md:block">
            {l}
          </a>
        ))}
        <div className="ml-auto flex items-center gap-1">
          <a href={REPO} className="flex items-center gap-1.5 rounded-md px-2.5 py-1 text-ink-2 transition-colors hover:bg-fill-2 hover:text-ink-1" aria-label="GitHub repository">
            <GitHubMark className="h-4 w-4" />
            <span className="hidden sm:inline">GitHub</span>
          </a>
          {/* the tray item, as the app draws it */}
          <a href="#top" className="flex items-center gap-1.5 rounded-md bg-fill-2 px-2.5 py-1 font-medium" title="What the tray item shows">
            <Flame className="h-3.5 w-3.5" solid="currentColor" />
            <span className="tnum font-mono text-[12.5px]">{fmtTokens(live)}</span>
          </a>
          <span className="hidden pl-2 text-ink-2 lg:inline">{clock}</span>
        </div>
      </nav>
    </header>
  )
}

function Hero({ live }: { live: number }) {
  return (
    <section id="top" className="grain relative isolate flex min-h-[100svh] items-center overflow-hidden pt-20 pb-16">
      <div className="absolute inset-0 -z-10">
        <LiquidEther
          colors={['#e5344a', '#ff7a3a', '#ffc95c']}
          intensity={1.25}
          mouseForce={24}
          cursorSize={140}
          persistence={0.82}
          autoDemo
          autoSpeed={0.5}
          autoIntensity={3}
          resolution={0.5}
          className="h-full w-full"
        />
        <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(140%_100%_at_55%_40%,transparent_35%,rgb(12_10_11/0.6)_75%,var(--color-bg)_100%)]" />
        <div className="pointer-events-none absolute inset-0 bg-[linear-gradient(90deg,rgb(12_10_11/0.6)_0%,rgb(12_10_11/0.25)_45%,transparent_70%)] max-lg:bg-[linear-gradient(180deg,rgb(12_10_11/0.5),rgb(12_10_11/0.2)_50%,transparent)]" />
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-40 bg-gradient-to-b from-transparent to-bg" />
      </div>

      <div className="mx-auto grid w-full max-w-6xl items-center gap-14 px-4 lg:grid-cols-[1fr_auto] lg:gap-12 sm:px-6">
        <div className="pointer-events-none">
          <a
            href={DOWNLOAD}
            className="pointer-events-auto inline-flex items-center gap-2 rounded-full border-[0.5px] border-white/15 bg-black/30 py-1 pr-3 pl-1 text-[12.5px] text-ink-2 backdrop-blur-md transition-colors hover:text-ink-1"
          >
            <span className="rounded-full bg-flame/90 px-2 py-0.5 text-[11.5px] font-semibold text-black">v0.1.0</span>
            Free and open source for macOS 14+
            <span aria-hidden>→</span>
          </a>

          <h1 className="mt-6 text-[clamp(44px,7.6vw,88px)] leading-[0.95] font-semibold tracking-[-0.045em]">
            <BlurText text="See every token" animateBy="words" delay={90} trigger="load" className="block" />
            <BlurText text="you burn." animateBy="words" delay={90} trigger="load" className="block text-ember" />
          </h1>

          <div className="mt-7 max-w-xl text-[clamp(17px,1.9vw,20px)] leading-relaxed text-ink-2">
            Tokens, API-equivalent cost and live plan limits for{' '}
            <RotatingText
              texts={TOOLS.map((t) => t.name)}
              mainClassName="inline-flex overflow-hidden rounded-lg bg-white/10 px-2 py-0.5 align-middle font-medium text-ink-1 backdrop-blur"
              staggerFrom="last"
              staggerDuration={0.02}
              rotationInterval={2200}
              transition={{ type: 'spring', damping: 30, stiffness: 400 }}
              initial={{ y: '100%' }}
              animate={{ y: 0 }}
              exit={{ y: '-120%' }}
              splitLevelClassName="overflow-hidden pb-0.5"
            />{' '}
            in one menu-bar panel.
          </div>

          <div className="pointer-events-auto mt-9 flex flex-wrap items-center gap-3">
            <Magnet padding={60} magnetStrength={4}>
              <a
                href={DOWNLOAD}
                className="flex items-center gap-2.5 rounded-full bg-ink-1 px-6 py-3.5 text-[15px] font-semibold text-black shadow-[0_10px_40px_-10px_rgb(255_122_58/0.8)] transition-transform active:scale-[0.97]"
              >
                <AppleMark className="h-[17px] w-[15px]" />
                Download for macOS
              </a>
            </Magnet>
            <a
              href={REPO}
              className="flex items-center gap-2 rounded-full border-[0.5px] border-white/20 bg-black/25 px-5 py-3.5 text-[15px] font-medium backdrop-blur-md transition-colors hover:bg-white/10"
            >
              <GitHubMark className="h-4 w-4" />
              Source on GitHub
            </a>
          </div>
          <div className="mt-5 text-[13px] text-ink-3">Universal .dmg · Apple Silicon and Intel · 6.6 MB · MIT</div>
        </div>

        <div className="flex justify-center lg:justify-end">
          <AnimatedContent trigger="mount" distance={60} duration={1.1} delay={0.35} blur={10} scale={0.96}>
            <Panel live={live} />
          </AnimatedContent>
        </div>
      </div>
    </section>
  )
}

function ToolStrip() {
  return (
    <section className="relative border-y-[0.5px] border-hair bg-bg py-8" aria-label="Supported tools">
      <div className="mb-5 text-center text-[13px] text-ink-3">Reads the logs and limits of</div>
      <LogoLoop
        logos={TOOLS.map((t) => ({
          title: t.name,
          node: (
            <span className="flex items-center gap-2.5 text-[20px] font-medium tracking-tight text-ink-2">
              <span className="h-2.5 w-2.5 rounded-full" style={{ background: t.color }} />
              {t.name}
            </span>
          ),
        }))}
        speed={45}
        gap={56}
        logoHeight={28}
        pauseOnHover
        fadeOut
        fadeOutColor="#0c0a0b"
        ariaLabel="Supported AI coding tools"
      />
    </section>
  )
}

function Features() {
  return (
    <section id="features" className="mx-auto max-w-6xl px-4 py-28 sm:px-6">
      <SectionHead kicker="Features" title="One click instead of five dashboards.">
        You pay for Claude Max, a ChatGPT plan, Cursor and maybe SuperGrok. Each hides its usage on a different page with
        its own idea of a limit. TokenHell reads all of them.
      </SectionHead>

      <div className="mt-16 grid gap-4 md:grid-cols-6">
        <Reveal className="md:col-span-4">
          <Card className="h-full">
            <CardTitle title="Tokens burned">
              Today, 7 days, 30 days or all time, with a daily sparkline split by app and the input, output, cache read and
              cache write mix.
            </CardTitle>
            <div className="mt-8 flex flex-wrap items-end gap-x-10 gap-y-6">
              <div>
                <div className="tnum text-[56px] leading-none font-semibold tracking-[-0.04em]">
                  <CountUp to={2.41} decimals={2} duration={1.6} />B
                </div>
                <div className="mt-2 text-[13px] text-ink-3">tokens in 30 days</div>
              </div>
              <div className="flex h-[72px] min-w-[200px] flex-1 items-end gap-[4px]">
                {Array.from({ length: 22 }, (_, k) => {
                  const v = (0.35 + 0.65 * Math.abs(Math.sin(k * 1.7))) * (k % 6 === 5 ? 0.25 : 1)
                  return (
                    <div key={k} className="bar-rise flex flex-1 flex-col-reverse overflow-hidden rounded-[3px]" style={{ height: `${v * 100}%`, animationDelay: `${k * 30}ms` }}>
                      {['claude', 'codex', 'cursor', 'grok'].map((a, i) => (
                        <span key={a} style={{ flexGrow: 0.35 + Math.abs(Math.sin(k * 1.1 + i * 1.4)), background: `var(--color-${a})` }} />
                      ))}
                    </div>
                  )
                })}
              </div>
            </div>
          </Card>
        </Reveal>

        <Reveal className="md:col-span-2" delay={0.08}>
          <Card className="h-full">
            <CardTitle title="Priced per request">
              What the same usage would cost on the API. The Opus total matches a hand calculation from Anthropic's rates to
              the cent.
            </CardTitle>
            <div className="mt-6 flex flex-wrap gap-2">
              {['1h cache writes', 'Long-context tiers', 'Fast mode', 'Priority and flex', 'Web search'].map((t) => (
                <span key={t} className="rounded-full border-[0.5px] border-hair bg-fill-1 px-3 py-1 text-[12.5px] text-ink-2">
                  {t}
                </span>
              ))}
            </div>
          </Card>
        </Reveal>

        <Reveal className="md:col-span-3" delay={0.04}>
          <Card className="h-full">
            <CardTitle title="Live plan limits">
              Claude's 5h and weekly windows (Opus-only too), Codex 5h and week, Cursor's month and Grok credits, each with a
              countdown to reset.
            </CardTitle>
            <div className="mt-7 space-y-3">
              <Gauge label="Claude · 5h" pct={59} />
              <Gauge label="Claude · Opus" pct={93} delay={120} />
              <Gauge label="Codex · week" pct={54} delay={240} />
              <Gauge label="Cursor · month" pct={38} delay={360} />
            </div>
          </Card>
        </Reveal>

        <Reveal className="md:col-span-3" delay={0.1}>
          <Card className="h-full">
            <CardTitle title="A tray icon that runs hot">
              When any limit crosses your threshold, the flame fills in. Show tokens, limit %, dollars or just the icon.
            </CardTitle>
            <div className="mt-7 grid grid-cols-2 gap-3">
              {[
                { t: 'Normal', hot: false, v: '111.3M' },
                { t: 'Hot at 85%', hot: true, v: '93%' },
              ].map((s) => (
                <div key={s.t} className="rounded-2xl border-[0.5px] border-hair bg-black/30 p-3">
                  <div className="flex h-8 items-center justify-end gap-3 rounded-lg bg-white/[0.06] px-3 text-[13px]">
                    <span className="flex items-center gap-1.5 font-medium">
                      {s.hot ? (
                        <Flame className="h-4 w-4" />
                      ) : (
                        <svg viewBox="0 0 100 100" className="h-4 w-4" aria-hidden>
                          <path d="M50 96C29 96 15 82 17 63 19 46 35 39 38 24 40 14 39 7 37 3 54 10 64 25 61 41 67 37 69 31 69 25 81 38 86 52 83 66 80 84 67 96 50 96Z" fill="none" stroke="currentColor" strokeWidth="7" />
                        </svg>
                      )}
                      <span className="tnum font-mono text-[12.5px]">{s.v}</span>
                    </span>
                  </div>
                  <div className="mt-2.5 text-center text-[12px] text-ink-3">{s.t}</div>
                </div>
              ))}
            </div>
          </Card>
        </Reveal>

        <Reveal className="md:col-span-2" delay={0.04}>
          <Card className="h-full">
            <CardTitle title="Daily token goal">
              An optional target across every tool. Resets at local midnight and sends one notification when you hit it.
            </CardTitle>
            <div className="mt-7">
              <div className="flex justify-between text-[13px]">
                <span className="text-ink-2">Today</span>
                <span className="tnum">76.3M / 100M</span>
              </div>
              <div className="mt-2 h-[6px] overflow-hidden rounded-full bg-fill-2">
                <span className="bar-grow block h-full w-[76%] rounded-full bg-flame" />
              </div>
            </div>
          </Card>
        </Reveal>

        <Reveal className="md:col-span-2" delay={0.08}>
          <Card className="h-full">
            <CardTitle title="Real Liquid Glass">
              A native macOS 26 glass window, not a CSS imitation. Falls back to vibrancy on older systems, in light and dark.
            </CardTitle>
            <div className="relative mt-7 h-[64px] overflow-hidden rounded-2xl bg-[conic-gradient(from_200deg_at_30%_60%,#e5344a,#ff7a3a,#ffc95c,#3987e5,#e5344a)]">
              <div className="glass absolute inset-2 flex items-center justify-center rounded-xl text-[13px] font-medium">
                glass.liquidGlassRegular
              </div>
            </div>
          </Card>
        </Reveal>

        <Reveal className="md:col-span-2" delay={0.12}>
          <Card className="h-full">
            <CardTitle title="Featherweight">
              Watches files with FSEvents and reads only new lines, so it sits at 0% CPU when idle.
            </CardTitle>
            <div className="mt-6 flex gap-8">
              <div>
                <div className="tnum text-[40px] leading-none font-semibold tracking-tight">
                  <CountUp to={6.6} decimals={1} duration={1.4} />
                  <span className="ml-1 text-[18px] text-ink-3">MB</span>
                </div>
                <div className="mt-1.5 text-[12px] text-ink-3">app size</div>
              </div>
              <div>
                <div className="tnum text-[40px] leading-none font-semibold tracking-tight">
                  0<span className="ml-0.5 text-[18px] text-ink-3">%</span>
                </div>
                <div className="mt-1.5 text-[12px] text-ink-3">CPU when idle</div>
              </div>
            </div>
          </Card>
        </Reveal>
      </div>
    </section>
  )
}

function Compare() {
  const rows = [
    ['Tools covered', '10: CLI logs plus Cursor, Grok and Copilot APIs', 'Usually one'],
    ['Pricing', 'Per request, with every tier and cache TTL', 'Tokens × one flat rate'],
    ['Live plan limits', 'Claude, Codex, Cursor, Grok, Copilot', 'Rarely'],
    ['History', 'Kept after tools delete old logs', 'Lost after 30 days'],
    ['Footprint', 'Native tray app, about 6 MB', 'Electron or a terminal tab'],
  ]
  return (
    <section className="mx-auto max-w-4xl px-4 pb-28 sm:px-6">
      <SectionHead kicker="Why it's different" title="Built for people on three plans at once." />
      <Reveal className="mt-12">
        <div className="overflow-hidden rounded-3xl border-[0.5px] border-hair bg-bg-2">
          <div className="grid grid-cols-[1fr_1.4fr_1fr] gap-4 border-b-[0.5px] border-hair px-5 py-3.5 text-[12.5px] text-ink-3 sm:px-7">
            <span />
            <span className="flex items-center gap-1.5 font-semibold text-ink-1">
              <Flame className="h-3.5 w-3.5" /> TokenHell
            </span>
            <span>Typical trackers</span>
          </div>
          {rows.map(([k, a, b], i) => (
            <div key={k} className={`grid grid-cols-[1fr_1.4fr_1fr] gap-4 px-5 py-4 text-[14px] sm:px-7 ${i ? 'border-t-[0.5px] border-hair' : ''}`}>
              <span className="text-ink-2">{k}</span>
              <span className="text-ink-1">{a}</span>
              <span className="text-ink-3">{b}</span>
            </div>
          ))}
        </div>
      </Reveal>
    </section>
  )
}

function Sources() {
  const rows = [
    { tool: 'Claude Code', color: 'var(--color-claude)', history: '~/.claude/projects/**/*.jsonl', limits: 'OAuth usage endpoint, token from Keychain' },
    { tool: 'Codex', color: 'var(--color-codex)', history: '~/.codex/sessions', limits: 'codex app-server → account/rateLimits/read' },
    { tool: 'Cursor', color: 'var(--color-cursor)', history: 'Dashboard usage events + optional CLI stop hook', limits: 'usage-summary, plus Grok weekly allowance' },
    { tool: 'Grok', color: 'var(--color-grok)', history: 'Grok Build updates.jsonl, grok-cli grok.db', limits: 'SuperGrok credits' },
    { tool: 'Copilot', color: 'var(--color-copilot)', history: '—', limits: 'copilot_internal/user' },
    { tool: 'Gemini CLI, OpenCode, Qwen, Kimi, Pi', color: 'var(--color-gemini)', history: 'Local session files and SQLite', limits: '—' },
  ]
  return (
    <section id="sources" className="relative border-t-[0.5px] border-hair bg-bg-2/60 py-28">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <SectionHead kicker="Where the numbers come from" title="Straight from the files your tools already write.">
          Claude and Codex are priced per request from the LiteLLM price map, refreshed weekly. Cursor and Grok report their
          own cost per request, and TokenHell uses that number.
        </SectionHead>
        <div className="mt-14 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {rows.map((r, i) => (
            <Reveal key={r.tool} delay={i * 0.05}>
              <div className="h-full rounded-2xl border-[0.5px] border-hair bg-bg p-5 transition-colors hover:border-white/15">
                <div className="flex items-center gap-2 font-semibold">
                  <span className="h-2 w-2 rounded-full" style={{ background: r.color }} />
                  {r.tool}
                </div>
                <dl className="mt-4 space-y-3 text-[13px]">
                  <div>
                    <dt className="text-ink-3">Token history</dt>
                    <dd className="mt-1 font-mono text-[12.5px] break-words text-ink-1">{r.history}</dd>
                  </div>
                  <div>
                    <dt className="text-ink-3">Limits</dt>
                    <dd className="mt-1 font-mono text-[12.5px] break-words text-ink-1">{r.limits}</dd>
                  </div>
                </dl>
              </div>
            </Reveal>
          ))}
        </div>
        <Reveal className="mt-6 text-center text-[13.5px] text-ink-3">
          All dollar figures are API-equivalent, not your subscription bill. The research behind each integration is in{' '}
          <a className="text-ink-2 underline decoration-white/20 underline-offset-4 hover:text-ink-1" href={`${REPO}/tree/main/docs/research`}>
            docs/research
          </a>
          .
        </Reveal>
      </div>
    </section>
  )
}

function Privacy() {
  const items = [
    {
      title: 'Local only',
      body: 'No server, no analytics, no telemetry. The only network calls go to the providers’ own usage endpoints and the LiteLLM price list.',
    },
    {
      title: 'Numbers only',
      body: 'Token counts, model names and timestamps in a local SQLite file. Prompts and responses are never stored, not even by the Cursor hook.',
    },
    {
      title: 'Credentials stay in memory',
      body: 'Read from the Keychain when needed, never written to disk, never refreshed, never sent anywhere but the provider they belong to.',
    },
  ]
  return (
    <section id="privacy" className="mx-auto max-w-6xl px-4 py-28 sm:px-6">
      <SectionHead kicker="Privacy" title="Everything stays on this Mac." />
      <div className="mt-14 grid gap-4 md:grid-cols-3">
        {items.map((it, i) => (
          <Reveal key={it.title} delay={i * 0.08}>
            <Card className="h-full">
              <div className="mb-5 font-mono text-[13px] text-flame">0{i + 1}</div>
              <CardTitle title={it.title}>{it.body}</CardTitle>
            </Card>
          </Reveal>
        ))}
      </div>
    </section>
  )
}

function Install() {
  return (
    <section id="install" className="relative overflow-hidden border-t-[0.5px] border-hair py-28">
      <div className="pointer-events-none absolute top-0 left-1/2 -z-10 h-[520px] w-[900px] -translate-x-1/2 rounded-full bg-[radial-gradient(closest-side,rgb(255_122_58/0.16),transparent)]" />
      <div className="mx-auto max-w-3xl px-4 sm:px-6">
        <Reveal className="text-center">
          <img src={appIcon} alt="TokenHell app icon" className="mx-auto h-24 w-24 drop-shadow-[0_20px_40px_rgb(229_52_74/0.35)]" />
          <h2 className="mt-6 text-[clamp(32px,5vw,52px)] leading-[1.05] font-semibold tracking-[-0.035em]">Install in a minute.</h2>
          <p className="mt-4 text-[17px] text-ink-2">Runs on Apple Silicon and Intel, macOS 14 or later.</p>
          <div className="mt-8 flex justify-center">
            <Magnet padding={60} magnetStrength={4}>
              <a
                href={DOWNLOAD}
                className="flex items-center gap-2.5 rounded-full bg-ink-1 px-7 py-4 text-[16px] font-semibold text-black shadow-[0_10px_40px_-10px_rgb(255_122_58/0.8)] transition-transform active:scale-[0.97]"
              >
                <AppleMark className="h-[18px] w-[16px]" />
                Download TokenHell_0.1.0_universal.dmg
              </a>
            </Magnet>
          </div>
        </Reveal>

        <div className="mt-16 space-y-10">
          <Reveal>
            <div className="flex gap-4">
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-fill-2 font-mono text-[12px]">1</span>
              <div className="min-w-0 flex-1">
                <h3 className="font-semibold">Drag it to Applications</h3>
                <p className="mt-1 text-[14.5px] text-ink-2">Open the .dmg and drag TokenHell into your Applications folder.</p>
              </div>
            </div>
          </Reveal>
          <Reveal>
            <div className="flex gap-4">
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-fill-2 font-mono text-[12px]">2</span>
              <div className="min-w-0 flex-1">
                <h3 className="font-semibold">Allow the first launch</h3>
                <p className="mt-1 text-[14.5px] text-ink-2">
                  The build is ad-hoc signed but not notarized, so macOS blocks it once. Click <b className="font-medium text-ink-1">Open Anyway</b> in
                  System Settings → Privacy & Security, or run:
                </p>
                <div className="mt-3">
                  <Code>{'$ xattr -dr com.apple.quarantine /Applications/TokenHell.app'}</Code>
                </div>
              </div>
            </div>
          </Reveal>
          <Reveal>
            <div className="flex gap-4">
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-fill-2 font-mono text-[12px]">3</span>
              <div className="min-w-0 flex-1">
                <h3 className="font-semibold">Approve the Keychain prompt once</h3>
                <p className="mt-1 text-[14.5px] text-ink-2">
                  Live limits for Claude and Cursor need their login tokens. TokenHell reads them through /usr/bin/security, so
                  one Always Allow survives updates. Deny it and it stays quiet for 30 minutes.
                </p>
              </div>
            </div>
          </Reveal>
          <Reveal>
            <div className="rounded-3xl border-[0.5px] border-hair bg-bg-2 p-5 sm:p-6">
              <h3 className="font-semibold">Or build it yourself</h3>
              <p className="mt-1 text-[14.5px] text-ink-2">About two minutes with Rust, Node 20+, pnpm and Xcode Command Line Tools.</p>
              <div className="mt-4">
                <Code>{`$ git clone ${REPO} && cd tokenhell\n$ pnpm install\n$ pnpm tauri build --bundles app\n$ open target/release/bundle/macos/TokenHell.app`}</Code>
              </div>
            </div>
          </Reveal>
        </div>
      </div>
    </section>
  )
}

function Footer() {
  return (
    <footer className="border-t-[0.5px] border-hair">
      <div className="mx-auto flex max-w-6xl flex-col gap-6 px-4 py-10 text-[13px] text-ink-3 sm:flex-row sm:items-center sm:px-6">
        <div className="flex items-center gap-2.5">
          <Flame className="h-4 w-4" />
          <span className="text-ink-2">TokenHell</span>
          <span>· MIT licensed</span>
        </div>
        <p className="sm:ml-auto sm:text-right">
          Want Windsurf, Zed, Amp or Cline?{' '}
          <a href={REPO} className="text-ink-2 underline decoration-white/20 underline-offset-4 hover:text-ink-1">
            Adapters are welcome
          </a>
          .
        </p>
      </div>
    </footer>
  )
}

export default function App() {
  const live = useLiveTokens()
  return (
    <>
      <MenuBar live={live} />
      <main>
        <Hero live={live} />
        <ToolStrip />
        <Features />
        <Compare />
        <Sources />
        <Privacy />
        <Install />
      </main>
      <Footer />
    </>
  )
}
