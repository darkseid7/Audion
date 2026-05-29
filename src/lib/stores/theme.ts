// Theme store - manages app theming and customization
import { writable, derived, get } from 'svelte/store';

export type ThemeMode = 'dark' | 'light' | 'system' | 'cyberpunk-neon' | 'aurora-borealis' | 'sunset-warmth' | 'ocean-depths' | 'sakura-bloom' | 'midnight-purple' | 'forest-grove' | 'monochrome';

export interface ThemeColors {
    accent: string;
    accentHover: string;
}

export interface ThemeState {
    mode: ThemeMode;
    accentColor: string;
    customAccentColors: string[];
}

// Preset accent colors
export const presetAccents = [
    { name: 'Green', color: '#1DB954' },
    { name: 'Blue', color: '#1E90FF' },
    { name: 'Purple', color: '#9B59B6' },
    { name: 'Pink', color: '#E91E63' },
    { name: 'Orange', color: '#FF6B35' },
    { name: 'Teal', color: '#00BCD4' },
    { name: 'Red', color: '#E74C3C' },
    { name: 'Yellow', color: '#F1C40F' },
];

// Theme presets (beyond dark/light/system)
export interface ThemePreset {
    id: ThemeMode;
    name: string;
    icon: string;
    description: string;
    accent: string;
    preview: { bg: string; accent: string; text: string };
}

export const themePresets: ThemePreset[] = [
    {
        id: 'cyberpunk-neon',
        name: 'Cyberpunk Neon',
        icon: '⚡',
        description: 'Electric neon with futuristic vibes',
        accent: '#ff00ff',
        preview: { bg: '#0a0a12', accent: '#ff00ff', text: '#00ffff' },
    },
    {
        id: 'aurora-borealis',
        name: 'Aurora Borealis',
        icon: '🌌',
        description: 'Mystical northern lights glow',
        accent: '#22d3ee',
        preview: { bg: '#0c1222', accent: '#22d3ee', text: '#a855f7' },
    },
    {
        id: 'sunset-warmth',
        name: 'Sunset Warmth',
        icon: '🌅',
        description: 'Cozy warm vibes like a summer sunset',
        accent: '#f59e0b',
        preview: { bg: '#1a1512', accent: '#f59e0b', text: '#ef4444' },
    },
    {
        id: 'ocean-depths',
        name: 'Ocean Depths',
        icon: '🌊',
        description: 'Deep sea blues with glowing accents',
        accent: '#0ea5e9',
        preview: { bg: '#0a1628', accent: '#0ea5e9', text: '#38bdf8' },
    },
    {
        id: 'sakura-bloom',
        name: 'Sakura Bloom',
        icon: '🌸',
        description: 'Elegant pink cherry blossom theme',
        accent: '#f472b6',
        preview: { bg: '#1a1520', accent: '#f472b6', text: '#f9a8d4' },
    },
    {
        id: 'midnight-purple',
        name: 'Midnight Purple',
        icon: '🔮',
        description: 'Elegant deep purple mystique',
        accent: '#a855f7',
        preview: { bg: '#0f0a1a', accent: '#a855f7', text: '#c084fc' },
    },
    {
        id: 'forest-grove',
        name: 'Forest Grove',
        icon: '🌲',
        description: 'Calming forest nature theme',
        accent: '#22c55e',
        preview: { bg: '#0a1410', accent: '#22c55e', text: '#4ade80' },
    },
    {
        id: 'monochrome',
        name: 'Monochrome',
        icon: '⬛',
        description: 'Elegant grayscale aesthetic',
        accent: '#ffffff',
        preview: { bg: '#0a0a0a', accent: '#ffffff', text: '#a3a3a3' },
    },
];

// Theme definitions: CSS variables + custom styles for each preset
interface ThemeDefinition {
    vars: Record<string, string>;
    customStyles: string;
}

const themeDefinitions: Record<string, ThemeDefinition> = {
    'cyberpunk-neon': {
        vars: {
            '--bg-base': '#0a0a12',
            '--bg-elevated': '#12121f',
            '--bg-surface': '#1a1a2e',
            '--bg-highlight': '#25254a',
            '--bg-press': '#33336a',
            '--accent-primary': '#ff00ff',
            '--accent-hover': '#ff44ff',
            '--accent-subtle': 'rgba(255, 0, 255, 0.15)',
            '--text-primary': '#ffffff',
            '--text-secondary': '#00ffff',
            '--text-subdued': '#6b6b9a',
            '--border-color': 'rgba(255, 0, 255, 0.3)',
        },
        customStyles: `
            .player-bar {
                background: linear-gradient(180deg, rgba(255,0,255,0.05) 0%, #0a0a12 100%) !important;
                border-top: 1px solid rgba(255,0,255,0.25) !important;
                box-shadow: 0 -4px 24px rgba(255,0,255,0.15) !important;
            }
            .sidebar, .nav-sidebar {
                border-right: 1px solid rgba(0,255,255,0.15) !important;
            }
            .btn-primary {
                background: linear-gradient(135deg, #ff00ff 0%, #00ffff 100%) !important;
                box-shadow: 0 0 16px rgba(255,0,255,0.4), 0 0 32px rgba(0,255,255,0.2) !important;
                text-shadow: 0 0 8px rgba(0,0,0,0.5) !important;
                color: #fff !important;
            }
            .btn-primary:hover {
                box-shadow: 0 0 24px rgba(255,0,255,0.6), 0 0 48px rgba(0,255,255,0.35) !important;
            }
            .icon-btn:hover {
                background: rgba(255,0,255,0.15) !important;
                box-shadow: 0 0 12px rgba(255,0,255,0.3) !important;
            }
            .icon-btn.active {
                color: #00ffff !important;
                text-shadow: 0 0 8px rgba(0,255,255,0.6) !important;
            }
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #00ffff !important;
                text-shadow: 0 0 6px rgba(0,255,255,0.4) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: repeating-linear-gradient(0deg, transparent, transparent 2px, rgba(0,255,255,0.015) 2px, rgba(0,255,255,0.015) 4px);
                z-index: 9998;
            }
        `
    },
    'aurora-borealis': {
        vars: {
            '--bg-base': '#0c1222',
            '--bg-elevated': '#111827',
            '--bg-surface': '#1e293b',
            '--bg-highlight': '#334155',
            '--bg-press': '#475569',
            '--accent-primary': '#22d3ee',
            '--accent-hover': '#67e8f9',
            '--accent-subtle': 'rgba(34, 211, 238, 0.15)',
            '--text-primary': '#f8fafc',
            '--text-secondary': '#94a3b8',
            '--text-subdued': '#64748b',
            '--border-color': 'rgba(34, 211, 238, 0.2)',
        },
        customStyles: `
            .player-bar {
                background: linear-gradient(180deg, rgba(34,211,238,0.05) 0%, rgba(168,85,247,0.03) 50%, #0c1222 100%) !important;
                border-top: 1px solid rgba(34,211,238,0.15) !important;
            }
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0c1222 0%, rgba(168,85,247,0.08) 50%, rgba(34,211,238,0.05) 100%) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at top left, rgba(34,211,238,0.08) 0%, transparent 50%),
                    radial-gradient(ellipse at top right, rgba(168,85,247,0.08) 0%, transparent 50%),
                    radial-gradient(ellipse at bottom, rgba(16,185,129,0.05) 0%, transparent 40%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #22d3ee 0%, #a855f7 50%, #10b981 100%) !important;
                background-size: 200% 200% !important;
                animation: aurora-shift 5s ease infinite !important;
            }
            @keyframes aurora-shift {
                0% { background-position: 0% 50%; }
                50% { background-position: 100% 50%; }
                100% { background-position: 0% 50%; }
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #22d3ee, #a855f7, #10b981) !important;
                background-size: 200% !important;
                animation: aurora-shift 3s linear infinite !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                background: linear-gradient(90deg, #22d3ee, #a855f7) !important;
                -webkit-background-clip: text !important;
                -webkit-text-fill-color: transparent !important;
                background-clip: text !important;
            }
        `
    },
    'sunset-warmth': {
        vars: {
            '--bg-base': '#1a1512',
            '--bg-elevated': '#231c17',
            '--bg-surface': '#2d2520',
            '--bg-highlight': '#3d332b',
            '--bg-press': '#4a3f35',
            '--accent-primary': '#f59e0b',
            '--accent-hover': '#fbbf24',
            '--accent-subtle': 'rgba(245, 158, 11, 0.15)',
            '--text-primary': '#fef3c7',
            '--text-secondary': '#d6c4a5',
            '--text-subdued': '#8b7355',
            '--border-color': 'rgba(245, 158, 11, 0.25)',
        },
        customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #1a1512 0%, rgba(245,158,11,0.08) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(239,68,68,0.05) 0%, rgba(245,158,11,0.08) 50%, #1a1512 100%) !important;
                border-top: 1px solid rgba(245,158,11,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                bottom: 0;
                left: 0;
                width: 100%;
                height: 60%;
                pointer-events: none;
                background: linear-gradient(0deg, rgba(245,158,11,0.04) 0%, rgba(239,68,68,0.02) 40%, transparent 100%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #f59e0b 0%, #ef4444 100%) !important;
                box-shadow: 0 4px 20px rgba(245,158,11,0.3) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #ef4444, #f59e0b, #fbbf24) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #fbbf24 !important;
            }
            .icon-btn.active {
                color: #f59e0b !important;
            }
        `
    },
    'ocean-depths': {
        vars: {
            '--bg-base': '#0a1628',
            '--bg-elevated': '#0f1d32',
            '--bg-surface': '#162544',
            '--bg-highlight': '#1e3356',
            '--bg-press': '#274068',
            '--accent-primary': '#0ea5e9',
            '--accent-hover': '#38bdf8',
            '--accent-subtle': 'rgba(14, 165, 233, 0.15)',
            '--text-primary': '#e0f2fe',
            '--text-secondary': '#7dd3fc',
            '--text-subdued': '#4b7c9e',
            '--border-color': 'rgba(14, 165, 233, 0.25)',
        },
        customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0a1628 0%, #0f1d32 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(14,165,233,0.08) 0%, #0a1628 100%) !important;
                border-top: 1px solid rgba(14,165,233,0.3) !important;
            }
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(circle at 20% 80%, rgba(14,165,233,0.08) 0%, transparent 15%),
                    radial-gradient(circle at 80% 20%, rgba(14,165,233,0.06) 0%, transparent 12%),
                    radial-gradient(circle at 50% 50%, rgba(14,165,233,0.04) 0%, transparent 20%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #0284c7 0%, #0ea5e9 50%, #38bdf8 100%) !important;
                box-shadow: 0 4px 25px rgba(14,165,233,0.4) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #0284c7, #0ea5e9, #38bdf8) !important;
                box-shadow: 0 0 15px rgba(14,165,233,0.5) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #38bdf8 !important;
                text-shadow: 0 0 8px rgba(56,189,248,0.5) !important;
            }
        `
    },
    'sakura-bloom': {
        vars: {
            '--bg-base': '#1a1520',
            '--bg-elevated': '#211c28',
            '--bg-surface': '#2a2433',
            '--bg-highlight': '#3a3245',
            '--bg-press': '#4a4055',
            '--accent-primary': '#f472b6',
            '--accent-hover': '#f9a8d4',
            '--accent-subtle': 'rgba(244, 114, 182, 0.15)',
            '--text-primary': '#fdf2f8',
            '--text-secondary': '#f9a8d4',
            '--text-subdued': '#9d7a8c',
            '--border-color': 'rgba(244, 114, 182, 0.25)',
        },
        customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #1a1520 0%, rgba(244,114,182,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(244,114,182,0.08) 0%, #1a1520 100%) !important;
                border-top: 1px solid rgba(244,114,182,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at top right, rgba(244,114,182,0.08) 0%, transparent 40%),
                    radial-gradient(ellipse at bottom left, rgba(249,168,212,0.05) 0%, transparent 35%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #ec4899 0%, #f472b6 50%, #f9a8d4 100%) !important;
                box-shadow: 0 4px 20px rgba(244,114,182,0.35) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #ec4899, #f472b6, #f9a8d4) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #f9a8d4 !important;
            }
            .icon-btn.active {
                color: #f472b6 !important;
            }
        `
    },
    'midnight-purple': {
        vars: {
            '--bg-base': '#0f0a1a',
            '--bg-elevated': '#150f24',
            '--bg-surface': '#1e1530',
            '--bg-highlight': '#2a1f42',
            '--bg-press': '#362a54',
            '--accent-primary': '#a855f7',
            '--accent-hover': '#c084fc',
            '--accent-subtle': 'rgba(168, 85, 247, 0.15)',
            '--text-primary': '#faf5ff',
            '--text-secondary': '#d8b4fe',
            '--text-subdued': '#7c5da0',
            '--border-color': 'rgba(168, 85, 247, 0.25)',
        },
        customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0f0a1a 0%, rgba(168,85,247,0.08) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(168,85,247,0.1) 0%, #0f0a1a 100%) !important;
                border-top: 1px solid rgba(168,85,247,0.25) !important;
            }
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(ellipse at center, rgba(168,85,247,0.05) 0%, transparent 60%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #7c3aed 0%, #a855f7 50%, #c084fc 100%) !important;
                box-shadow: 0 4px 25px rgba(168,85,247,0.4) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #7c3aed, #a855f7, #c084fc) !important;
                box-shadow: 0 0 12px rgba(168,85,247,0.5) !important;
            }
            .track-item:hover {
                background: linear-gradient(90deg, rgba(168,85,247,0.1), transparent) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #c084fc !important;
                text-shadow: 0 0 8px rgba(192,132,252,0.4) !important;
            }
        `
    },
    'forest-grove': {
        vars: {
            '--bg-base': '#0a1410',
            '--bg-elevated': '#0f1c15',
            '--bg-surface': '#15261d',
            '--bg-highlight': '#1e3327',
            '--bg-press': '#274032',
            '--accent-primary': '#22c55e',
            '--accent-hover': '#4ade80',
            '--accent-subtle': 'rgba(34, 197, 94, 0.15)',
            '--text-primary': '#ecfdf5',
            '--text-secondary': '#86efac',
            '--text-subdued': '#4d7c5e',
            '--border-color': 'rgba(34, 197, 94, 0.25)',
        },
        customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0a1410 0%, rgba(34,197,94,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(34,197,94,0.08) 0%, #0a1410 100%) !important;
                border-top: 1px solid rgba(34,197,94,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                bottom: 0;
                left: 0;
                width: 100%;
                height: 50%;
                pointer-events: none;
                background: linear-gradient(0deg, rgba(34,197,94,0.04) 0%, transparent 100%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #16a34a 0%, #22c55e 50%, #4ade80 100%) !important;
                box-shadow: 0 4px 20px rgba(34,197,94,0.35) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #16a34a, #22c55e, #4ade80) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #4ade80 !important;
            }
        `
    },
    'monochrome': {
        vars: {
            '--bg-base': '#0a0a0a',
            '--bg-elevated': '#141414',
            '--bg-surface': '#1f1f1f',
            '--bg-highlight': '#2a2a2a',
            '--bg-press': '#3a3a3a',
            '--accent-primary': '#ffffff',
            '--accent-hover': '#e5e5e5',
            '--accent-subtle': 'rgba(255, 255, 255, 0.1)',
            '--text-primary': '#ffffff',
            '--text-secondary': '#a3a3a3',
            '--text-subdued': '#525252',
            '--border-color': '#2a2a2a',
        },
        customStyles: `
            .sidebar, .nav-sidebar {
                background: #0a0a0a !important;
                border-right: 1px solid #1f1f1f !important;
            }
            .player-bar {
                background: #0a0a0a !important;
                border-top: 1px solid #1f1f1f !important;
            }
            .btn-primary {
                background: #ffffff !important;
                color: #0a0a0a !important;
            }
            .btn-primary:hover {
                background: #e5e5e5 !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: #ffffff !important;
            }
            .icon-btn.active {
                color: #ffffff !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #ffffff !important;
            }
        `
    },
};

const THEME_STORAGE_KEY = 'rlist_theme';

// Default theme state
const defaultTheme: ThemeState = {
    mode: 'dark',
    accentColor: '#1DB954',
    customAccentColors: [],
};

// Load theme from localStorage
function loadTheme(): ThemeState {
    if (typeof window === 'undefined') return defaultTheme;

    try {
        const stored = localStorage.getItem(THEME_STORAGE_KEY);
        if (stored) {
            return { ...defaultTheme, ...JSON.parse(stored) };
        }
    } catch (error) {
        console.error('[Theme] Failed to load:', error);
    }

    return defaultTheme;
}

// Save theme to localStorage
function saveTheme(state: ThemeState): void {
    if (typeof window === 'undefined') return;

    try {
        localStorage.setItem(THEME_STORAGE_KEY, JSON.stringify(state));
    } catch (error) {
        console.error('[Theme] Failed to save:', error);
    }
}

// Create theme store
function createThemeStore() {
    // Start with default theme - actual theme is loaded in initialize() when browser is ready
    const { subscribe, set, update } = writable<ThemeState>(defaultTheme);

    return {
        subscribe,

        setMode(mode: ThemeMode) {
            update(state => {
                const newState = { ...state, mode };
                saveTheme(newState);
                applyTheme(newState);
                return newState;
            });
        },

        setAccentColor(color: string) {
            update(state => {
                const newState = { ...state, accentColor: color };
                saveTheme(newState);
                applyTheme(newState);
                return newState;
            });
        },

        addCustomColor(color: string) {
            update(state => {
                if (state.customAccentColors.includes(color)) return state;
                const newColors = [...state.customAccentColors, color].slice(-5); // Keep last 5
                const newState = { ...state, customAccentColors: newColors };
                saveTheme(newState);
                return newState;
            });
        },

        initialize() {
            const state = loadTheme();
            set(state);
            applyTheme(state);
        }
    };
}

export const theme = createThemeStore();

// Lighten a color for hover state
function lightenColor(hex: string, percent: number): string {
    const num = parseInt(hex.replace('#', ''), 16);
    const amt = Math.round(2.55 * percent);
    const R = Math.min(255, (num >> 16) + amt);
    const G = Math.min(255, ((num >> 8) & 0x00FF) + amt);
    const B = Math.min(255, (num & 0x0000FF) + amt);
    return `#${(1 << 24 | R << 16 | G << 8 | B).toString(16).slice(1)}`;
}

// Darken a color
function darkenColor(hex: string, percent: number): string {
    const num = parseInt(hex.replace('#', ''), 16);
    const amt = Math.round(2.55 * percent);
    const R = Math.max(0, (num >> 16) - amt);
    const G = Math.max(0, ((num >> 8) & 0x00FF) - amt);
    const B = Math.max(0, (num & 0x0000FF) - amt);
    return `#${(1 << 24 | R << 16 | G << 8 | B).toString(16).slice(1)}`;
}

// Convert hex to RGB string (r, g, b)
function hexToRgb(hex: string): string {
    const num = parseInt(hex.replace('#', ''), 16);
    const R = (num >> 16);
    const G = ((num >> 8) & 0x00FF);
    const B = (num & 0x0000FF);
    return `${R}, ${G}, ${B}`;
}

// ID for the injected custom theme stylesheet
const CUSTOM_THEME_STYLE_ID = 'audion-custom-theme-styles';

function removeCustomThemeStyles(): void {
    const existing = document.getElementById(CUSTOM_THEME_STYLE_ID);
    if (existing) existing.remove();
}

function injectCustomThemeStyles(css: string): void {
    removeCustomThemeStyles();
    const style = document.createElement('style');
    style.id = CUSTOM_THEME_STYLE_ID;
    style.textContent = css;
    document.head.appendChild(style);
}

// Apply theme to CSS variables
export function applyTheme(state: ThemeState): void {
    if (typeof document === 'undefined') return;

    const root = document.documentElement;

    // Handle preset themes (data-driven)
    const definition = themeDefinitions[state.mode];
    if (definition) {
        for (const [key, value] of Object.entries(definition.vars)) {
            root.style.setProperty(key, value);
        }
        // Set accent RGB for rgba() usage
        const accent = definition.vars['--accent-primary'];
        if (accent && accent.startsWith('#')) {
            root.style.setProperty('--accent-primary-rgb', hexToRgb(accent));
        }
        root.setAttribute('data-theme', state.mode);
        if (definition.customStyles) {
            injectCustomThemeStyles(definition.customStyles);
        } else {
            removeCustomThemeStyles();
        }
        return;
    }

    // Standard themes (dark/light/system) — remove any custom preset styles
    removeCustomThemeStyles();

    const isDark = state.mode === 'dark' ||
        (state.mode === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

    // Apply accent colors
    root.style.setProperty('--accent-primary', state.accentColor);
    root.style.setProperty('--accent-primary-rgb', hexToRgb(state.accentColor));
    root.style.setProperty('--accent-hover', lightenColor(state.accentColor, 15));
    root.style.setProperty('--accent-subtle', state.accentColor + '20');

    if (isDark) {
        // Dark theme
        root.style.setProperty('--bg-base', '#121212');
        root.style.setProperty('--bg-elevated', '#181818');
        root.style.setProperty('--bg-surface', '#282828');
        root.style.setProperty('--bg-highlight', '#3e3e3e');
        root.style.setProperty('--bg-press', '#535353');
        root.style.setProperty('--text-primary', '#ffffff');
        root.style.setProperty('--text-secondary', '#b3b3b3');
        root.style.setProperty('--text-subdued', '#6a6a6a');
        root.style.setProperty('--border-color', '#404040');
    } else {
        // Light theme
        root.style.setProperty('--bg-base', '#f5f5f5');
        root.style.setProperty('--bg-elevated', '#ffffff');
        root.style.setProperty('--bg-surface', '#e8e8e8');
        root.style.setProperty('--bg-highlight', '#d4d4d4');
        root.style.setProperty('--bg-press', '#c0c0c0');
        root.style.setProperty('--text-primary', '#121212');
        root.style.setProperty('--text-secondary', '#535353');
        root.style.setProperty('--text-subdued', '#8a8a8a');
        root.style.setProperty('--border-color', '#d0d0d0');
    }

    // Add theme attribute to root for CSS selectors
    root.setAttribute('data-theme', isDark ? 'dark' : 'light');
}

// Derived store for current theme mode
export const isDarkMode = derived(theme, $theme => {
    // All preset themes are dark-based
    if ($theme.mode in themeDefinitions) return true;
    if ($theme.mode === 'system') {
        if (typeof window === 'undefined') return true;
        return window.matchMedia('(prefers-color-scheme: dark)').matches;
    }
    return $theme.mode === 'dark';
});
