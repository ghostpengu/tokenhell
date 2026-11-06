import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { mockInvoke } from './mock'

export type Range = 'today' | '7d' | '30d' | 'all'
export type Provider =
  | 'claude' | 'codex' | 'cursor' | 'grok' | 'gemini'
  | 'copilot' | 'opencode' | 'pi' | 'qwen' | 'kimi'
export type TitleMode = 'icon' | 'tokens' | 'limit' | 'cost'

export interface Tokens { total: number; input: number; output: number; cache_read: number; cache_write: number }
export interface ModelSummary { model: string; tokens: Tokens; cost_usd: number | null; events: number }
export interface ProviderSummary {
  provider: Provider; label: string; tokens: Tokens; cost_usd: number; cost_partial: boolean; models: ModelSummary[]
}
export interface DayApp { provider: Provider; label: string; tokens: number; cost_usd: number }
export interface DayPoint { day: string; tokens: number; cost_usd: number; apps: DayApp[] }
export interface Summary {
  range: Range; tokens: Tokens; cost_usd: number; cost_partial: boolean
  providers: ProviderSummary[]; daily: DayPoint[]; prices_updated: number | null; generated_at: number
}
export interface LimitWindow { key: string; label: string; used_pct: number | null; resets_at: number | null; detail: string | null }
export interface ResetCredits { available: number; next_expires_at: number | null }
export interface LimitReport {
  provider: Provider; windows: LimitWindow[]; plan: string | null; notes: string[]
  reset_credits?: ResetCredits | null
  observed_at: number; stale: boolean; error: string | null
}
export type ResetOutcome = 'reset' | 'nothingToReset' | 'noCredit' | 'alreadyRedeemed'
export interface Settings {
  title_mode: TitleMode; limits: Partial<Record<Provider, boolean>>
  poll_minutes: number; hot_threshold: number; price_refresh: boolean
}
export interface ProviderInfo { id: Provider; label: string; detected: boolean; limits_supported: boolean; limits_enabled: boolean }
export interface SettingsView { settings: Settings; providers: ProviderInfo[]; launch_at_login: boolean; version: string }

export const inTauri = '__TAURI_INTERNALS__' in window

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return inTauri ? invoke<T>(cmd, args) : mockInvoke<T>(cmd, args)
}

export const api = {
  summary: (range: Range) => call<Summary>('get_summary', { range }),
  limits: () => call<LimitReport[]>('get_limits'),
  refresh: () => call<void>('refresh'),
  codexConsumeReset: (idempotencyKey: string) => call<ResetOutcome>('codex_consume_reset', { idempotencyKey }),
  settings: () => call<SettingsView>('get_settings'),
  setSettings: (settings: Settings) => call<void>('set_settings', { settings }),
  setLaunchAtLogin: (enabled: boolean) => call<void>('set_launch_at_login', { enabled }),
  hide: () => call<void>('hide_panel'),
  quit: () => call<void>('quit'),
}

export async function on(event: string, fn: () => void): Promise<UnlistenFn> {
  return inTauri ? listen(event, fn) : () => {}
}

export const PROVIDER_LABEL: Record<Provider, string> = {
  claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor', grok: 'Grok', gemini: 'Gemini CLI',
  copilot: 'Copilot', opencode: 'OpenCode', pi: 'Pi', qwen: 'Qwen Code', kimi: 'Kimi',
}
