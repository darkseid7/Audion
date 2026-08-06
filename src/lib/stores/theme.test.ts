/**
 * SDD: player-5-new-themes — Strict TDD unit tests
 *
 * Tests verify: registration, definition integrity, WCAG AA contrast
 * compliance (all 45 text-on-background pairs ≥ 4.5:1), accent-primary
 * matches fixed spec, and zero hex reuse from existing 22 themes.
 */
import { describe, it, expect } from 'vitest';
import { themePresets, themeDefinitions } from './theme';

// ── Constants ───────────────────────────────────────────────────────────

const NEW_IDS = [
  'neon-lime',
  'golden-hour',
  'indigo-night',
  'coral-reef',
  'frosted-amethyst',
] as const;

const FIXED_ACCENTS: Record<string, string> = {
  'neon-lime': '#7FFF00',
  'golden-hour': '#FFD700',
  'indigo-night': '#6A0DAD',
  'coral-reef': '#FF7F50',
  'frosted-amethyst': '#9966CC',
};

const TEXT_TOKENS = ['--text-primary', '--text-secondary', '--text-subdued'] as const;
const BG_TOKENS = ['--bg-base', '--bg-elevated', '--bg-surface'] as const;

// ── WCAG AA Relative Luminance & Contrast Ratio ─────────────────────────

/**
 * Parse a 6-digit hex colour (#RRGGBB) into sRGB channels [0,255].
 * Case-insensitive.
 */
function parseHex(hex: string): [number, number, number] {
  const h = hex.replace('#', '');
  const r = parseInt(h.substring(0, 2), 16);
  const g = parseInt(h.substring(2, 4), 16);
  const b = parseInt(h.substring(4, 6), 16);
  return [r, g, b];
}

/**
 * WCAG 2.1 relative luminance of a sRGB channel component (0–1).
 */
function srgbToLinear(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/**
 * WCAG 2.1 relative luminance from a #RRGGBB hex string.
 */
function relativeLuminance(hex: string): number {
  const [r, g, b] = parseHex(hex);
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b);
}

/**
 * WCAG 2.1 contrast ratio between two luminance values.
 * Returns a value ≥ 1 (lighter/darker).
 */
function contrastRatio(lum1: number, lum2: number): number {
  const lighter = Math.max(lum1, lum2);
  const darker = Math.min(lum1, lum2);
  return (lighter + 0.05) / (darker + 0.05);
}

// ── Stub helpers (fail early when themes aren't registered yet) ─────────

function getDef(id: string) {
  const def = themeDefinitions[id];
  if (!def) {
    throw new Error(
      `Theme definition "${id}" not registered yet. ` +
        'This is expected in RED phase — implement themes to fix.',
    );
  }
  return def;
}

// ── Test: Registration ──────────────────────────────────────────────────

describe('Theme registration — themePresets[]', () => {
  NEW_IDS.forEach((id) => {
    it(`should register preset: ${id}`, () => {
      const found = themePresets.find((p) => p.id === id);
      expect(found).toBeDefined();
    });
  });
});

// ── Test: Definitions ───────────────────────────────────────────────────

describe('Theme definitions — themeDefinitions{}', () => {
  NEW_IDS.forEach((id) => {
    it(`should have definition with 12 var keys: ${id}`, () => {
      const def = getDef(id);
      const keys = Object.keys(def.vars);
      expect(keys).toHaveLength(12);
      // Verify the 12 expected token names exist
      expect(def.vars['--bg-base']).toBeDefined();
      expect(def.vars['--bg-elevated']).toBeDefined();
      expect(def.vars['--bg-surface']).toBeDefined();
      expect(def.vars['--bg-highlight']).toBeDefined();
      expect(def.vars['--bg-press']).toBeDefined();
      expect(def.vars['--accent-primary']).toBeDefined();
      expect(def.vars['--accent-hover']).toBeDefined();
      expect(def.vars['--accent-subtle']).toBeDefined();
      expect(def.vars['--text-primary']).toBeDefined();
      expect(def.vars['--text-secondary']).toBeDefined();
      expect(def.vars['--text-subdued']).toBeDefined();
      expect(def.vars['--border-color']).toBeDefined();
    });
  });
});

// ── Test: Accent-primary matches fixed spec ─────────────────────────────

describe('Accent-primary matches fixed spec', () => {
  NEW_IDS.forEach((id) => {
    const expected = FIXED_ACCENTS[id];
    it(`${id} → --accent-primary = ${expected}`, () => {
      const def = getDef(id);
      expect(def.vars['--accent-primary']).toBe(expected);
    });
  });
});

// ── Test: WCAG AA contrast ≥ 4.5:1 for all text-on-background pairs ─────

describe('WCAG AA contrast ≥ 4.5:1', () => {
  NEW_IDS.forEach((id) => {
    TEXT_TOKENS.forEach((textToken) => {
      BG_TOKENS.forEach((bgToken) => {
        it(`${id}: ${textToken} vs ${bgToken} ≥ 4.5:1`, () => {
          const def = getDef(id);

          const textHex = def.vars[textToken];
          const bgHex = def.vars[bgToken];

          // Only test solid hex tokens; skip rgba() values
          if (!textHex.startsWith('#') || !bgHex.startsWith('#')) {
            // This pair shouldn't be tested (e.g. accent-subtle vs bg)
            // But we're only iterating TEXT_TOKENS × BG_TOKENS — all should be hex
            throw new Error(
              `Expected solid hex for ${textToken}/${bgToken}, got "${textHex}" / "${bgHex}"`,
            );
          }

          const textLum = relativeLuminance(textHex);
          const bgLum = relativeLuminance(bgHex);
          const ratio = contrastRatio(textLum, bgLum);

          expect(ratio).toBeGreaterThanOrEqual(4.5);
        });
      });
    });
  });
});

// ── Test: Zero hex overlap with existing 22 themes ──────────────────────

describe('Zero hex overlap with existing themes', () => {
  it('should have no solid hex reuse from the 22 existing themes', () => {
    // Ensure all 5 new themes are registered before comparison
    NEW_IDS.forEach((id) => {
      expect(themeDefinitions[id]).toBeDefined();
    });

    const existingIds = Object.keys(themeDefinitions).filter(
      (k) => !(NEW_IDS as readonly string[]).includes(k),
    );

    // Collect all solid hex literals from existing themes (lowercased)
    const existingHexes = new Set<string>();
    for (const existingId of existingIds) {
      const def = themeDefinitions[existingId];
      for (const value of Object.values(def.vars)) {
        if (value.startsWith('#')) {
          existingHexes.add(value.toLowerCase());
        }
      }
    }

    // Collect all solid hex literals from the 5 new themes
    const newHexEntries: { theme: string; varName: string; hex: string }[] = [];
    for (const id of NEW_IDS) {
      const def = themeDefinitions[id];
      for (const [varName, value] of Object.entries(def.vars)) {
        if (value.startsWith('#')) {
          newHexEntries.push({ theme: id, varName, hex: value.toLowerCase() });
        }
      }
    }

    // Intersection check
    const overlapping = newHexEntries.filter((e) => existingHexes.has(e.hex));

    if (overlapping.length > 0) {
      const details = overlapping
        .map((e) => `${e.theme}::${e.varName}=${e.hex}`)
        .join(', ');
      throw new Error(`Hex overlap with existing themes: ${details}`);
    }

    // Sanity: we should have found at least 5×9 = 45 solid hexes
    // (5 themes × 9 solid tokens; accent-subtle and border-color are rgba)
    expect(newHexEntries.length).toBeGreaterThanOrEqual(45);
  });
});
