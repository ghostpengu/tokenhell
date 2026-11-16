<script lang="ts">
  import { api, type SettingsView, type TitleMode } from './api'
  import { parseTokenAmount } from './format'
  import { PROVIDER_COLOR } from './colors'

  let { onback }: { onback: () => void } = $props()
  let goalInput = $state('')
  let goalError = $state('')
  let goalStatus = $state('')
  let goalSaving = $state(false)
  let view = $state<SettingsView | null>(null)

  $effect(() => {
    api.settings().then(async (v) => {
      view = v
      goalInput = v.settings.daily_token_goal?.toString() ?? ''
      if (v.settings.daily_token_goal != null) await notificationStatus(false)
    }).catch((e) => (goalError = String(e)))
  })

  async function save() {
    if (view) await api.setSettings($state.snapshot(view.settings))
  }
  async function notificationStatus(request: boolean) {
    try {
      const granted = await api.goalNotificationPermission(request)
      goalStatus = granted ? 'Notification enabled when you reach your goal.' : 'Notifications are off. Enable them in macOS System Settings; progress still works.'
    } catch {
      goalStatus = 'Notifications unavailable; progress still works.'
    }
  }
  async function saveGoal(disable = false) {
    if (!view || goalSaving) return
    const amount = parseTokenAmount(goalInput)
    goalError = ''
    if (!disable && amount === null) {
      goalError = 'Enter a positive token amount, e.g. 100M, 1.5B, or 500K (maximum 9,007,199,254,740,991 tokens).'
      return
    }
    goalSaving = true
    try {
      if (!disable) await notificationStatus(true)
      const settings = { ...$state.snapshot(view.settings), daily_token_goal: disable ? null : amount }
      await api.setSettings(settings)
      view.settings = settings
      if (disable) { goalInput = ''; goalStatus = 'Daily goal disabled.' }
      else goalStatus = `Goal saved. ${goalStatus}`
    } catch (e) { goalError = `Could not save goal: ${String(e)}` }
    finally { goalSaving = false }
  }
  function setMode(m: TitleMode) {
    if (!view) return
    view.settings.title_mode = m
    save()
  }
  function toggleLimit(id: string, on: boolean) {
    if (!view) return
    view.settings.limits = { ...view.settings.limits, [id]: on }
    const p = view.providers.find((p) => p.id === id)
    if (p) p.limits_enabled = on
    save()
  }
  async function toggleLogin(on: boolean) {
    if (!view) return
    view.launch_at_login = on
    await api.setLaunchAtLogin(on)
  }
  const MODES: { id: TitleMode; label: string }[] = [
    { id: 'icon', label: 'Icon' }, { id: 'tokens', label: 'Tokens' }, { id: 'limit', label: 'Limit %' }, { id: 'cost', label: '$' },
  ]
</script>

<div class="settings">
  <header>
    <button class="back" onclick={onback} aria-label="Back">
      <svg width="8" height="13" viewBox="0 0 8 13"><path d="M6.5 1.5 1.5 6.5l5 5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>
    <h1>Settings</h1>
  </header>

  {#if view}
    <section>
      <div class="eyebrow">Menu bar shows</div>
      <div class="seg" role="radiogroup">
        {#each MODES as m}
          <button role="radio" aria-checked={view.settings.title_mode === m.id} class:on={view.settings.title_mode === m.id} onclick={() => setMode(m.id)}>{m.label}</button>
        {/each}
      </div>
    </section>

    <section>
      <div class="eyebrow">Daily goal</div>
      <label class="goal-input">
        <span>Tokens per day</span>
        <input type="text" bind:value={goalInput} placeholder="e.g. 100M or 1B" disabled={goalSaving} />
      </label>
      <div class="goal-actions">
        <button onclick={() => saveGoal()} disabled={goalSaving}>Save goal</button>
        {#if view.settings.daily_token_goal != null}<button onclick={() => saveGoal(true)} disabled={goalSaving}>Disable</button>{/if}
      </div>
      <p class="fine">All tools combined, including cache tokens. Resets at local midnight.</p>
      {#if goalError}<p class="fine goal-error" role="alert">{goalError}</p>{/if}
      {#if goalStatus}<p class="fine" role="status">{goalStatus}</p>{/if}
    </section>

    <section>
      <div class="eyebrow">Plan limits</div>
      <div class="list">
        {#each view.providers as p (p.id)}
          <label class="item">
            <span class="dot" style="background:{PROVIDER_COLOR[p.id]}"></span>
            <span class="lab">{p.label}</span>
            {#if !p.detected}<span class="hint">not found</span>{/if}
            <input type="checkbox" class="switch" checked={p.limits_enabled} onchange={(e) => toggleLimit(p.id, e.currentTarget.checked)} />
          </label>
        {/each}
      </div>
      <p class="fine">Claude reads its login from the Keychain (macOS asks once). Tokens are only read, never refreshed or stored.</p>
    </section>

    <section>
      <div class="eyebrow">General</div>
      <div class="list">
        <label class="item">
          <span class="lab">Check limits every</span>
          <select bind:value={view.settings.poll_minutes} onchange={save}>
            {#each [1, 2, 5, 10, 15, 30] as m}<option value={m}>{m} min</option>{/each}
          </select>
        </label>
        <label class="item">
          <span class="lab">Hot icon at</span>
          <select bind:value={view.settings.hot_threshold} onchange={save}>
            {#each [70, 80, 85, 90, 95, 101] as t}<option value={t}>{t > 100 ? 'never' : t + '%'}</option>{/each}
          </select>
        </label>
        <label class="item">
          <span class="lab">Refresh model prices weekly</span>
          <input type="checkbox" class="switch" bind:checked={view.settings.price_refresh} onchange={save} />
        </label>
        <label class="item">
          <span class="lab">Launch at login</span>
          <input type="checkbox" class="switch" checked={view.launch_at_login} onchange={(e) => toggleLogin(e.currentTarget.checked)} />
        </label>
      </div>
    </section>

    <footer>
      <span>TokenHell {view.version} · everything stays on this Mac</span>
      <button class="quit" onclick={() => api.quit()}>Quit</button>
    </footer>
  {/if}
</div>

<style>
  .goal-input { display: flex; gap: 10px; align-items: center; justify-content: space-between; }
  .goal-input input { min-width: 0; width: 155px; font: inherit; color: var(--ink-1); background: var(--fill-1); border: 1px solid var(--hair); border-radius: 6px; padding: 5px 7px; }
  .goal-actions { display: flex; gap: 8px; }
  .goal-actions button { background: var(--fill-2); padding: 4px 9px; border-radius: 6px; }
  .goal-actions button:disabled { opacity: .5; }
  .goal-error { color: var(--critical); }
  .settings { overflow-y: auto; display: flex; flex-direction: column; gap: 14px; height: 100%; }
  header { display: flex; align-items: center; gap: 10px; height: 28px; }
  h1 { font-size: 14px; font-weight: 650; margin: 0; }
  .back { width: 24px; height: 24px; display: grid; place-items: center; border-radius: 7px; color: var(--ink-2); }
  .back:hover { background: var(--fill-2); }
  section { display: flex; flex-direction: column; gap: 7px; }
  .seg { display: grid; grid-template-columns: repeat(4, 1fr); padding: 2px; border-radius: 9px; background: var(--fill-1); }
  .seg button { height: 24px; border-radius: 7px; font-size: 12px; color: var(--ink-2); }
  .seg button.on { background: var(--tip-bg); color: var(--ink-1); font-weight: 600; box-shadow: 0 1px 3px rgba(0,0,0,.12); }
  .list { border-radius: 11px; background: var(--fill-1); padding: 2px 10px; }
  .item { display: flex; align-items: center; gap: 8px; min-height: 32px; }
  .item + .item { border-top: 0.5px solid var(--hair); }
  .lab { flex: 1; }
  .hint { font-size: 11px; color: var(--ink-3); }
  select {
    font: inherit; font-size: 12px; color: var(--ink-1); background: var(--fill-2); border: 0; border-radius: 6px; padding: 2px 6px;
  }
  .switch {
    appearance: none; width: 30px; height: 18px; border-radius: 9px; background: var(--fill-2); position: relative;
    transition: background 150ms; margin: 0; flex: none;
  }
  .switch::after {
    content: ''; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%;
    background: white; box-shadow: 0 1px 2px rgba(0,0,0,.25); transition: transform 150ms;
  }
  .switch:checked { background: var(--good); }
  .switch:checked::after { transform: translateX(12px); }
  .fine { font-size: 11px; color: var(--ink-3); margin: 0 2px; }
  footer { margin-top: auto; display: flex; align-items: center; justify-content: space-between; font-size: 11px; color: var(--ink-3); }
  .quit { color: var(--ink-2); padding: 3px 8px; border-radius: 6px; }
  .quit:hover { background: var(--fill-2); color: var(--ink-1); }
</style>
