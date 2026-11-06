<script lang="ts">
  import { api, type SettingsView, type TitleMode } from './api'
  import { PROVIDER_COLOR } from './colors'

  let { onback }: { onback: () => void } = $props()
  let view = $state<SettingsView | null>(null)

  $effect(() => {
    api.settings().then((v) => (view = v))
  })

  async function save() {
    if (view) await api.setSettings($state.snapshot(view.settings))
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
  .settings { display: flex; flex-direction: column; gap: 14px; height: 100%; }
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
