<script context="module" lang="ts">
  import { themePresets, presetAccents, type ThemeMode, type ThemeState } from "$lib/stores/theme";
  export function selectThemeMode(mode: ThemeMode, change: (mode: ThemeMode) => void): void {
    if (mode === "dark" || mode === "light" || mode === "system" || themePresets.some(preset => preset.id === mode)) change(mode);
  }
  export function selectAccentColor(color: string, change: (color: string) => void): void {
    if (/^#[0-9A-Fa-f]{6}$/.test(color)) change(color);
  }
  export function addCustomAccent(color: string, change: (color: string) => void): void {
    if (/^#[0-9A-Fa-f]{6}$/.test(color)) change(color);
  }
</script>
<script lang="ts">
  import { _, locale } from "svelte-i18n";
  function translated(key: string, fallback: string): string { return $locale ? $_(key, { default: fallback }) : fallback; }
  export let state: ThemeState;
  export let onModeChange: (mode: ThemeMode) => void = () => {};
  export let onAccentChange: (color: string) => void = () => {};
  export let onCustomAccent: (color: string) => void = () => {};
  let customColor = "#1DB954";
</script>
<section class="settings-section appearance-settings" aria-labelledby="appearance-heading">
  <h2 id="appearance-heading" class="section-label">{translated('settings.language', 'Appearance')}</h2>
  <div class="settings-card">
    {#if $$slots.language}<slot name="language" /><div class="divider"></div>{/if}
           <div class="inner-section">
             <span class="setting-title">{translated('settings.themeMode', 'Theme mode')}</span>
             <div class="segmented-pill" style="margin-top: 6px;">
               <button class="segment-btn" data-theme-mode="dark" aria-pressed={state.mode === 'dark'} class:active={state.mode === 'dark'} on:click={() => selectThemeMode('dark', onModeChange)}>{translated('settings.dark', 'Dark')}</button>
               <button class="segment-btn" data-theme-mode="light" aria-pressed={state.mode === 'light'} class:active={state.mode === 'light'} on:click={() => selectThemeMode('light', onModeChange)}>{translated('settings.light', 'Light')}</button>
               <button class="segment-btn" data-theme-mode="system" aria-pressed={state.mode === 'system'} class:active={state.mode === 'system'} on:click={() => selectThemeMode('system', onModeChange)}>{translated('settings.system', 'System')}</button>
             </div>
           </div>

           <div class="divider"></div>

           <div class="inner-section">
             <span class="setting-title">{translated('settings.themePresets', 'Theme presets')}</span>
             <div class="theme-presets-grid" style="margin-top: 6px;">
               {#each themePresets as preset}
                 <button
                   class="theme-preset-card"
                   data-theme-mode={preset.id}
                   aria-pressed={state.mode === preset.id}
                   class:active={state.mode === preset.id}
                   on:click={() => selectThemeMode(preset.id, onModeChange)}
                   title={preset.description}
                 >
                   <div class="preset-preview" style="background: {preset.preview.bg};">
                     <span class="preset-icon" style="color: {preset.preview.accent}; text-shadow: 0 0 8px {preset.preview.accent};">{preset.icon}</span>
                     <div class="preset-colors">
                       <span class="preset-dot" style="background: {preset.preview.accent};"></span>
                       <span class="preset-dot" style="background: {preset.preview.text};"></span>
                     </div>
                   </div>
                   <span class="preset-name">{preset.name}</span>
                 </button>
               {/each}
             </div>
           </div>

           <slot name="after-presets" />

           <div class="divider"></div>

           {#if state.mode === 'dark' || state.mode === 'light' || state.mode === 'system'}
           <div class="inner-section">
             <span class="setting-title">Accent color</span>
             <div class="color-grid-compact" style="margin-top: 6px;">
               {#each presetAccents as preset}
                 <button
                   class="color-swatch-sm"
                   class:active={state.accentColor === preset.color}
                   style="background-color: {preset.color}"
                   on:click={() => selectAccentColor(preset.color, onAccentChange)}
                   title={preset.name}
                 ></button>
               {/each}
             </div>
           </div>

             <div class="divider"></div>
             <div class="inner-section">
               <span class="setting-title">Custom accent color</span>
               <div class="color-grid-compact">{#each state.customAccentColors as color}<button class="color-swatch-sm" class:active={state.accentColor === color} style:background-color={color} title={color} aria-label={color} aria-pressed={state.accentColor === color} on:click={() => selectAccentColor(color, onAccentChange)}></button>{/each}</div>
               <form class="custom-color-input" on:submit|preventDefault={() => addCustomAccent(customColor, onCustomAccent)}>
                 <input class="color-picker" type="color" aria-label="Pick custom accent color" bind:value={customColor} />
                 <input class="color-text" aria-label="Custom accent color" bind:value={customColor} pattern="#[0-9A-Fa-f]{6}" maxlength="7" />
                 <button class="add-btn" type="submit">Add</button>
               </form>
             </div>
           {/if}

  </div>
</section>
<style>


  .settings-section {
    margin-bottom: var(--spacing-xl);
  }


  .section-label {
    font-size: 0.75rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-subdued);
    margin-bottom: var(--spacing-sm);
    padding-left: var(--spacing-xs);
    opacity: 0.8;
  }


  .setting-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.2;
    display: block;
  }

  .custom-color-input {
    display: flex;
    gap: var(--spacing-sm);
    align-items: center;
  }


  .color-picker {
    width: 40px;
    height: 40px;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    padding: 0;
  }


  .color-picker::-webkit-color-swatch-wrapper {
    padding: 0;
  }


  .color-picker::-webkit-color-swatch {
    border: none;
    border-radius: var(--radius-sm);
  }


  .color-text {
    flex: 1;
    padding: var(--spacing-sm) var(--spacing-md);
    background-color: var(--bg-surface);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-family: monospace;
    max-width: 120px;
  }


  .color-text:focus {
    outline: none;
    border-color: var(--accent-primary);
  }


  .add-btn {
    padding: var(--spacing-sm) var(--spacing-md);
    background-color: var(--accent-primary);
    color: var(--bg-base);
    font-weight: 600;
    border-radius: var(--radius-sm);
    transition: all var(--transition-fast);
  }


  .add-btn:hover {
    background-color: var(--accent-hover);
  }


  .inner-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }


  .divider {
    height: 1px;
    background-color: var(--border-color);
    margin: var(--spacing-xs) 0;
    opacity: 0.5;
  }

  .segmented-pill {
    display: flex;
    background-color: var(--bg-highlight);
    padding: 4px;
    border-radius: var(--radius-full);
    gap: 2px;
    border: 1px solid var(--border-color);
  }


  .segment-btn {
    flex: 1;
    padding: 8px 12px;
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--text-secondary);
    border-radius: var(--radius-xl);
    transition: all var(--transition-fast);
    background: transparent;
    border: none;
    cursor: pointer;
    min-height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
  }


  .segment-btn:hover:not(.active) {
    background-color: rgba(255, 255, 255, 0.05);
    color: var(--text-primary);
  }


  .segment-btn.active {
    background-color: var(--bg-surface);
    color: var(--accent-primary);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
  }

  .color-grid-compact {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    padding: 4px 0;
  }

  .theme-presets-grid {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    padding: 4px 0;
  }


  .theme-preset-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 0;
    background: none;
    border: 2px solid transparent;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: border-color 0.2s, transform 0.15s;
  }


  .theme-preset-card:hover {
    transform: scale(1.05);
  }


  .theme-preset-card.active {
    border-color: var(--accent-primary);
  }


  .preset-preview {
    width: 80px;
    height: 50px;
    border-radius: var(--radius-sm);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    overflow: hidden;
  }


  .preset-icon {
    font-size: 1.1rem;
    line-height: 1;
  }


  .preset-colors {
    display: flex;
    gap: 4px;
  }


  .preset-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }


  .preset-name {
    font-size: 0.7rem;
    color: var(--text-secondary);
    white-space: nowrap;
  }


  .theme-preset-card.active .preset-name {
    color: var(--accent-primary);
  }


  .color-swatch-sm {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-full);
    cursor: pointer;
    border: 2px solid transparent;
    transition: transform 0.2s, border-color 0.2s;
    padding: 0;
  }


  .color-swatch-sm:hover {
    transform: scale(1.2);
  }


  .color-swatch-sm.active {
    border-color: var(--text-primary);
    box-shadow: 0 0 0 2px var(--bg-surface);
  }

  .settings-card {
    background-color: var(--bg-surface);
    border-radius: var(--radius-lg);
    border: 1px solid var(--border-color);
    padding: var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-lg);
  }


  .segmented-pill {
    display: flex;
    background-color: var(--bg-highlight);
    border-radius: var(--radius-lg);
    padding: 4px;
    border: 1px solid var(--border-color);
    width: 100%;
  }


  .segment-btn {
    flex: 1;
    background: none;
    border: none;
    padding: var(--spacing-sm);
    color: var(--text-secondary);
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: pointer;
    border-radius: calc(var(--radius-lg) - 2px);
    transition: all 0.2s ease;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 48px;
    gap: 4px;
  }


  .segment-btn.active {
    background-color: var(--bg-elevated);
    color: var(--accent-primary);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  }


  @media (max-width: 768px) {
    .settings-section {
      margin-bottom: var(--spacing-md);
      margin-left: -4px;
      margin-right: -4px;
    }
    button { min-height: 48px; }
  }

  button:focus-visible, input:focus-visible { outline: 2px solid var(--accent-primary); outline-offset: 2px; }
</style>
