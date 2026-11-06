import type { Provider } from './api'

// Fixed per-provider identity color (never re-assigned by rank or filtering).
export const PROVIDER_COLOR: Record<Provider, string> = {
  codex: 'var(--s1)', claude: 'var(--s2)', cursor: 'var(--s3)', grok: 'var(--s4)',
  gemini: 'var(--s5)', copilot: 'var(--s6)', opencode: 'var(--s7)', pi: 'var(--s8)',
  qwen: 'var(--ink-3)', kimi: 'var(--ink-3)',
}
