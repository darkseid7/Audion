// Theme store - manages app theming and customization
import { writable, derived, get } from "svelte/store";

export type ThemeMode =
  | "dark"
  | "light"
  | "system"
  | "cyberpunk-neon"
  | "aurora-borealis"
  | "sunset-warmth"
  | "ocean-depths"
  | "sakura-bloom"
  | "midnight-purple"
  | "forest-grove"
  | "monochrome"
  | "lava-flow"
  | "vaporwave"
  | "arctic-ice"
  | "cosmic-void"
  | "rose-gold"
  | "stormy-night"
  | "toxic-acid"
  | "crimson-dusk"
  | "mocha-espresso"
  | "cobalt-royal"
  | "terracotta"
  | "holo-foil"
  | "neon-lime"
  | "golden-hour"
  | "indigo-night"
  | "coral-reef"
  | "frosted-amethyst"
  | "true-black";

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
  { name: "Green", color: "#1DB954" },
  { name: "Blue", color: "#1E90FF" },
  { name: "Purple", color: "#9B59B6" },
  { name: "Pink", color: "#E91E63" },
  { name: "Orange", color: "#FF6B35" },
  { name: "Teal", color: "#00BCD4" },
  { name: "Red", color: "#E74C3C" },
  { name: "Yellow", color: "#F1C40F" },
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
    id: "cyberpunk-neon",
    name: "Cyberpunk Neon",
    icon: "⚡",
    description: "Electric neon with futuristic vibes",
    accent: "#ff00ff",
    preview: { bg: "#0a0a12", accent: "#ff00ff", text: "#00ffff" },
  },
  {
    id: "aurora-borealis",
    name: "Aurora Borealis",
    icon: "🌌",
    description: "Mystical northern lights glow",
    accent: "#22d3ee",
    preview: { bg: "#0c1222", accent: "#22d3ee", text: "#a855f7" },
  },
  {
    id: "sunset-warmth",
    name: "Sunset Warmth",
    icon: "🌅",
    description: "Cozy warm vibes like a summer sunset",
    accent: "#f59e0b",
    preview: { bg: "#1a1512", accent: "#f59e0b", text: "#ef4444" },
  },
  {
    id: "ocean-depths",
    name: "Ocean Depths",
    icon: "🌊",
    description: "Deep sea blues with glowing accents",
    accent: "#0ea5e9",
    preview: { bg: "#0a1628", accent: "#0ea5e9", text: "#38bdf8" },
  },
  {
    id: "sakura-bloom",
    name: "Sakura Bloom",
    icon: "🌸",
    description: "Elegant pink cherry blossom theme",
    accent: "#f472b6",
    preview: { bg: "#1a1520", accent: "#f472b6", text: "#f9a8d4" },
  },
  {
    id: "midnight-purple",
    name: "Midnight Purple",
    icon: "🔮",
    description: "Elegant deep purple mystique",
    accent: "#a855f7",
    preview: { bg: "#0f0a1a", accent: "#a855f7", text: "#c084fc" },
  },
  {
    id: "forest-grove",
    name: "Forest Grove",
    icon: "🌲",
    description: "Calming forest nature theme",
    accent: "#22c55e",
    preview: { bg: "#0a1410", accent: "#22c55e", text: "#4ade80" },
  },
  {
    id: "monochrome",
    name: "Monochrome",
    icon: "⬛",
    description: "Elegant grayscale aesthetic",
    accent: "#ffffff",
    preview: { bg: "#0a0a0a", accent: "#ffffff", text: "#a3a3a3" },
  },
  {
    id: "lava-flow",
    name: "Lava Flow",
    icon: "🌋",
    description: "Intense molten heat and crimson glow",
    accent: "#ef4444",
    preview: { bg: "#0f0808", accent: "#ef4444", text: "#f97316" },
  },
  {
    id: "vaporwave",
    name: "Vaporwave",
    icon: "🌴",
    description: "80s retro neon synthwave aesthetic",
    accent: "#f0abfc",
    preview: { bg: "#0d0a1f", accent: "#f0abfc", text: "#22d3ee" },
  },
  {
    id: "arctic-ice",
    name: "Arctic Ice",
    icon: "❄️",
    description: "Glacial cyan with frosted cool tones",
    accent: "#67e8f9",
    preview: { bg: "#0a1418", accent: "#67e8f9", text: "#a5f3fc" },
  },
  {
    id: "cosmic-void",
    name: "Cosmic Void",
    icon: "🌠",
    description: "Deep space darkness with violet starfield",
    accent: "#a78bfa",
    preview: { bg: "#050510", accent: "#a78bfa", text: "#c4b5fd" },
  },
  {
    id: "rose-gold",
    name: "Rose Gold",
    icon: "✨",
    description: "Luxurious champagne and pink-gold metallic",
    accent: "#fda4af",
    preview: { bg: "#1a1416", accent: "#fda4af", text: "#fbbf24" },
  },
  {
    id: "stormy-night",
    name: "Stormy Night",
    icon: "⛈️",
    description: "Moody slate with electric blue rain",
    accent: "#60a5fa",
    preview: { bg: "#0c1117", accent: "#60a5fa", text: "#93c5fd" },
  },
  {
    id: "toxic-acid",
    name: "Toxic Acid",
    icon: "☢️",
    description: "Aggressive radioactive lime hazard vibes",
    accent: "#a3e635",
    preview: { bg: "#0a1008", accent: "#a3e635", text: "#bef264" },
  },
  {
    id: "crimson-dusk",
    name: "Crimson Dusk",
    icon: "🍷",
    description: "Deep wine burgundy gothic atmosphere",
    accent: "#e11d48",
    preview: { bg: "#0c0608", accent: "#e11d48", text: "#fda4af" },
  },
  {
    id: "mocha-espresso",
    name: "Mocha Espresso",
    icon: "☕",
    description: "Warm coffee brown cozy sophistication",
    accent: "#d97706",
    preview: { bg: "#1a100a", accent: "#d97706", text: "#fbbf24" },
  },
  {
    id: "cobalt-royal",
    name: "Cobalt Royal",
    icon: "💎",
    description: "Pure sapphire jewel-tone regal blue",
    accent: "#2563eb",
    preview: { bg: "#0a0f1f", accent: "#2563eb", text: "#60a5fa" },
  },
  {
    id: "terracotta",
    name: "Terracotta",
    icon: "🏺",
    description: "Sun-baked clay earth tones and adobe warmth",
    accent: "#c2410c",
    preview: { bg: "#1a1108", accent: "#c2410c", text: "#fb923c" },
  },
  {
    id: "holo-foil",
    name: "Holo Foil",
    icon: "🪩",
    description: "Animated iridescent holographic — never the same color twice",
    accent: "#c084fc",
    preview: { bg: "#08080f", accent: "#ff00ff", text: "#00ffff" },
  },
  {
    id: "true-black",
    name: "True Black",
    icon: "🌑",
    description: "Maximum absence — pure OLED black, zero distraction",
    accent: "#ffffff",
    preview: { bg: "#000000", accent: "#ffffff", text: "#a3a3a3" },
  },
  {
    id: "neon-lime",
    name: "Neon Lime",
    icon: "💚",
    description: "Electric green neon with high-voltage energy",
    accent: "#7FFF00",
    preview: { bg: "#0d0d00", accent: "#7FFF00", text: "#bfff80" },
  },
  {
    id: "golden-hour",
    name: "Golden Hour",
    icon: "🟡",
    description: "Warm amber glow like a perfect sunset",
    accent: "#FFD700",
    preview: { bg: "#120e08", accent: "#FFD700", text: "#ffec8b" },
  },
  {
    id: "indigo-night",
    name: "Indigo Night",
    icon: "🟣",
    description: "Deep violet mystery and midnight calm",
    accent: "#6A0DAD",
    preview: { bg: "#08041a", accent: "#6A0DAD", text: "#c9a0f0" },
  },
  {
    id: "coral-reef",
    name: "Coral Reef",
    icon: "🪸",
    description: "Warm coral shimmer like tropical shallows",
    accent: "#FF7F50",
    preview: { bg: "#140a0a", accent: "#FF7F50", text: "#ffb89a" },
  },
  {
    id: "frosted-amethyst",
    name: "Frosted Amethyst",
    icon: "💜",
    description: "Cool purple crystal with soft frost glow",
    accent: "#9966CC",
    preview: { bg: "#0a0a14", accent: "#9966CC", text: "#c4a0e8" },
  },
];

// Theme definitions: CSS variables + custom styles for each preset
interface ThemeDefinition {
  vars: Record<string, string>;
  customStyles: string;
}

export const themeDefinitions: Record<string, ThemeDefinition> = {
  "cyberpunk-neon": {
    vars: {
      "--bg-base": "#0a0a12",
      "--bg-elevated": "#12121f",
      "--bg-surface": "#1a1a2e",
      "--bg-highlight": "#25254a",
      "--bg-press": "#33336a",
      "--accent-primary": "#ff00ff",
      "--accent-hover": "#ff44ff",
      "--accent-subtle": "rgba(255, 0, 255, 0.15)",
      "--text-primary": "#ffffff",
      "--text-secondary": "#00ffff",
      "--text-subdued": "#6b6b9a",
      "--border-color": "rgba(255, 0, 255, 0.3)",
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
        `,
  },
  "aurora-borealis": {
    vars: {
      "--bg-base": "#0c1222",
      "--bg-elevated": "#111827",
      "--bg-surface": "#1e293b",
      "--bg-highlight": "#334155",
      "--bg-press": "#475569",
      "--accent-primary": "#22d3ee",
      "--accent-hover": "#67e8f9",
      "--accent-subtle": "rgba(34, 211, 238, 0.15)",
      "--text-primary": "#f8fafc",
      "--text-secondary": "#94a3b8",
      "--text-subdued": "#64748b",
      "--border-color": "rgba(34, 211, 238, 0.2)",
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
        `,
  },
  "sunset-warmth": {
    vars: {
      "--bg-base": "#1a1512",
      "--bg-elevated": "#231c17",
      "--bg-surface": "#2d2520",
      "--bg-highlight": "#3d332b",
      "--bg-press": "#4a3f35",
      "--accent-primary": "#f59e0b",
      "--accent-hover": "#fbbf24",
      "--accent-subtle": "rgba(245, 158, 11, 0.15)",
      "--text-primary": "#fef3c7",
      "--text-secondary": "#d6c4a5",
      "--text-subdued": "#8b7355",
      "--border-color": "rgba(245, 158, 11, 0.25)",
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
        `,
  },
  "ocean-depths": {
    vars: {
      "--bg-base": "#0a1628",
      "--bg-elevated": "#0f1d32",
      "--bg-surface": "#162544",
      "--bg-highlight": "#1e3356",
      "--bg-press": "#274068",
      "--accent-primary": "#0ea5e9",
      "--accent-hover": "#38bdf8",
      "--accent-subtle": "rgba(14, 165, 233, 0.15)",
      "--text-primary": "#e0f2fe",
      "--text-secondary": "#7dd3fc",
      "--text-subdued": "#4b7c9e",
      "--border-color": "rgba(14, 165, 233, 0.25)",
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
        `,
  },
  "sakura-bloom": {
    vars: {
      "--bg-base": "#1a1520",
      "--bg-elevated": "#211c28",
      "--bg-surface": "#2a2433",
      "--bg-highlight": "#3a3245",
      "--bg-press": "#4a4055",
      "--accent-primary": "#f472b6",
      "--accent-hover": "#f9a8d4",
      "--accent-subtle": "rgba(244, 114, 182, 0.15)",
      "--text-primary": "#fdf2f8",
      "--text-secondary": "#f9a8d4",
      "--text-subdued": "#9d7a8c",
      "--border-color": "rgba(244, 114, 182, 0.25)",
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
        `,
  },
  "midnight-purple": {
    vars: {
      "--bg-base": "#0f0a1a",
      "--bg-elevated": "#150f24",
      "--bg-surface": "#1e1530",
      "--bg-highlight": "#2a1f42",
      "--bg-press": "#362a54",
      "--accent-primary": "#a855f7",
      "--accent-hover": "#c084fc",
      "--accent-subtle": "rgba(168, 85, 247, 0.15)",
      "--text-primary": "#faf5ff",
      "--text-secondary": "#d8b4fe",
      "--text-subdued": "#7c5da0",
      "--border-color": "rgba(168, 85, 247, 0.25)",
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
        `,
  },
  "forest-grove": {
    vars: {
      "--bg-base": "#0a1410",
      "--bg-elevated": "#0f1c15",
      "--bg-surface": "#15261d",
      "--bg-highlight": "#1e3327",
      "--bg-press": "#274032",
      "--accent-primary": "#22c55e",
      "--accent-hover": "#4ade80",
      "--accent-subtle": "rgba(34, 197, 94, 0.15)",
      "--text-primary": "#ecfdf5",
      "--text-secondary": "#86efac",
      "--text-subdued": "#4d7c5e",
      "--border-color": "rgba(34, 197, 94, 0.25)",
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
        `,
  },
  monochrome: {
    vars: {
      "--bg-base": "#0a0a0a",
      "--bg-elevated": "#141414",
      "--bg-surface": "#1f1f1f",
      "--bg-highlight": "#2a2a2a",
      "--bg-press": "#3a3a3a",
      "--accent-primary": "#ffffff",
      "--accent-hover": "#e5e5e5",
      "--accent-subtle": "rgba(255, 255, 255, 0.1)",
      "--text-primary": "#ffffff",
      "--text-secondary": "#a3a3a3",
      "--text-subdued": "#525252",
      "--border-color": "#2a2a2a",
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
        `,
  },
  "lava-flow": {
    vars: {
      "--bg-base": "#0f0808",
      "--bg-elevated": "#1a0d0a",
      "--bg-surface": "#261512",
      "--bg-highlight": "#3a201c",
      "--bg-press": "#4d2a24",
      "--accent-primary": "#ef4444",
      "--accent-hover": "#f87171",
      "--accent-subtle": "rgba(239, 68, 68, 0.15)",
      "--text-primary": "#fef2f2",
      "--text-secondary": "#fca5a5",
      "--text-subdued": "#8b5a5a",
      "--border-color": "rgba(239, 68, 68, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0f0808 0%, rgba(239,68,68,0.06) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(239,68,68,0.08) 0%, rgba(249,115,22,0.05) 50%, #0f0808 100%) !important;
                border-top: 1px solid rgba(239,68,68,0.3) !important;
                box-shadow: 0 -4px 20px rgba(239,68,68,0.15) !important;
            }
            body::after {
                content: '';
                position: fixed;
                bottom: 0;
                left: 0;
                width: 100%;
                height: 60%;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at 30% 100%, rgba(239,68,68,0.1) 0%, transparent 50%),
                    radial-gradient(ellipse at 70% 100%, rgba(249,115,22,0.07) 0%, transparent 45%);
                z-index: -1;
            }
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: repeating-linear-gradient(0deg, transparent, transparent 3px, rgba(239,68,68,0.02) 3px, rgba(239,68,68,0.02) 4px);
                z-index: 9998;
                opacity: 0.6;
            }
            .btn-primary {
                background: linear-gradient(135deg, #dc2626 0%, #ef4444 50%, #f97316 100%) !important;
                box-shadow: 0 4px 24px rgba(239,68,68,0.5), 0 0 16px rgba(249,115,22,0.3) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #dc2626, #ef4444, #f97316, #fbbf24) !important;
                box-shadow: 0 0 12px rgba(239,68,68,0.6) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #fca5a5 !important;
                text-shadow: 0 0 8px rgba(239,68,68,0.5) !important;
            }
            .icon-btn.active {
                color: #ef4444 !important;
                text-shadow: 0 0 6px rgba(239,68,68,0.4) !important;
            }
        `,
  },
  vaporwave: {
    vars: {
      "--bg-base": "#0d0a1f",
      "--bg-elevated": "#15102e",
      "--bg-surface": "#1d1740",
      "--bg-highlight": "#2b2356",
      "--bg-press": "#3a2f70",
      "--accent-primary": "#f0abfc",
      "--accent-hover": "#f5d0fe",
      "--accent-subtle": "rgba(240, 171, 252, 0.15)",
      "--text-primary": "#fdf4ff",
      "--text-secondary": "#e879f9",
      "--text-subdued": "#7a6198",
      "--border-color": "rgba(34, 211, 238, 0.25)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0d0a1f 0%, rgba(168,85,247,0.08) 50%, rgba(34,211,238,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(240,171,252,0.08) 0%, rgba(34,211,238,0.05) 50%, #0d0a1f 100%) !important;
                border-top: 1px solid rgba(34,211,238,0.3) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    linear-gradient(180deg, transparent 0%, transparent 55%, rgba(240,171,252,0.06) 75%, rgba(34,211,238,0.1) 100%),
                    radial-gradient(ellipse at 20% 30%, rgba(240,171,252,0.06) 0%, transparent 40%),
                    radial-gradient(ellipse at 80% 70%, rgba(34,211,238,0.06) 0%, transparent 40%);
                z-index: -1;
            }
            body::before {
                content: '';
                position: fixed;
                bottom: 0;
                left: 0;
                right: 0;
                height: 45%;
                pointer-events: none;
                background:
                    linear-gradient(0deg, transparent 0%, rgba(34,211,238,0.18) 100%);
                mask: linear-gradient(0deg, black 0%, transparent 100%);
                -webkit-mask: linear-gradient(0deg, black 0%, transparent 100%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #f0abfc 0%, #a855f7 50%, #22d3ee 100%) !important;
                box-shadow: 0 0 20px rgba(240,171,252,0.4), 0 0 40px rgba(34,211,238,0.2) !important;
                color: #0d0a1f !important;
                font-weight: 700 !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #f0abfc, #a855f7, #22d3ee) !important;
                box-shadow: 0 0 10px rgba(240,171,252,0.5) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                background: linear-gradient(90deg, #f0abfc, #22d3ee) !important;
                -webkit-background-clip: text !important;
                -webkit-text-fill-color: transparent !important;
                background-clip: text !important;
            }
            .icon-btn.active {
                color: #22d3ee !important;
                text-shadow: 0 0 6px rgba(34,211,238,0.5) !important;
            }
        `,
  },
  "arctic-ice": {
    vars: {
      "--bg-base": "#0a1418",
      "--bg-elevated": "#0f1d24",
      "--bg-surface": "#15272f",
      "--bg-highlight": "#1e353f",
      "--bg-press": "#28434f",
      "--accent-primary": "#67e8f9",
      "--accent-hover": "#a5f3fc",
      "--accent-subtle": "rgba(103, 232, 249, 0.15)",
      "--text-primary": "#f0f9ff",
      "--text-secondary": "#bae6fd",
      "--text-subdued": "#5a8090",
      "--border-color": "rgba(103, 232, 249, 0.25)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0a1418 0%, rgba(103,232,249,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(103,232,249,0.08) 0%, #0a1418 100%) !important;
                border-top: 1px solid rgba(103,232,249,0.25) !important;
                box-shadow: 0 -2px 16px rgba(103,232,249,0.1) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(circle at 15% 25%, rgba(103,232,249,0.05) 0%, transparent 12%),
                    radial-gradient(circle at 85% 15%, rgba(165,243,252,0.04) 0%, transparent 10%),
                    radial-gradient(circle at 75% 75%, rgba(103,232,249,0.04) 0%, transparent 15%),
                    radial-gradient(circle at 25% 85%, rgba(186,230,253,0.03) 0%, transparent 10%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #0891b2 0%, #67e8f9 50%, #a5f3fc 100%) !important;
                box-shadow: 0 4px 20px rgba(103,232,249,0.35) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #0891b2, #67e8f9, #a5f3fc, #ffffff) !important;
                box-shadow: 0 0 10px rgba(103,232,249,0.4) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #a5f3fc !important;
                text-shadow: 0 0 8px rgba(165,243,252,0.4) !important;
            }
            .icon-btn.active {
                color: #67e8f9 !important;
            }
        `,
  },
  "cosmic-void": {
    vars: {
      "--bg-base": "#050510",
      "--bg-elevated": "#0a0a18",
      "--bg-surface": "#101022",
      "--bg-highlight": "#1a1a30",
      "--bg-press": "#252540",
      "--accent-primary": "#a78bfa",
      "--accent-hover": "#c4b5fd",
      "--accent-subtle": "rgba(167, 139, 250, 0.15)",
      "--text-primary": "#f5f3ff",
      "--text-secondary": "#c4b5fd",
      "--text-subdued": "#5a5080",
      "--border-color": "rgba(167, 139, 250, 0.25)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #050510 0%, rgba(167,139,250,0.06) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(167,139,250,0.06) 0%, #050510 100%) !important;
                border-top: 1px solid rgba(167,139,250,0.2) !important;
            }
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(1px 1px at 20% 30%, #ffffff, transparent),
                    radial-gradient(1px 1px at 60% 70%, #ffffff, transparent),
                    radial-gradient(1.5px 1.5px at 80% 20%, #a78bfa, transparent),
                    radial-gradient(1px 1px at 30% 80%, #ffffff, transparent),
                    radial-gradient(1px 1px at 90% 60%, #c4b5fd, transparent),
                    radial-gradient(1px 1px at 10% 50%, #ffffff, transparent),
                    radial-gradient(1.5px 1.5px at 50% 40%, #a78bfa, transparent),
                    radial-gradient(1px 1px at 70% 90%, #ffffff, transparent);
                background-size: 250px 250px;
                background-repeat: repeat;
                opacity: 0.7;
                z-index: -1;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(ellipse at center, rgba(167,139,250,0.05) 0%, transparent 70%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #7c3aed 0%, #a78bfa 50%, #ddd6fe 100%) !important;
                box-shadow: 0 0 20px rgba(167,139,250,0.4), 0 0 40px rgba(196,181,253,0.2) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #7c3aed, #a78bfa, #ffffff) !important;
                box-shadow: 0 0 10px rgba(167,139,250,0.5) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #c4b5fd !important;
                text-shadow: 0 0 8px rgba(196,181,253,0.4) !important;
            }
            .icon-btn.active {
                color: #a78bfa !important;
            }
        `,
  },
  "rose-gold": {
    vars: {
      "--bg-base": "#1a1416",
      "--bg-elevated": "#221a1d",
      "--bg-surface": "#2b2225",
      "--bg-highlight": "#3a2d30",
      "--bg-press": "#4a3a3d",
      "--accent-primary": "#fda4af",
      "--accent-hover": "#fecdd3",
      "--accent-subtle": "rgba(253, 164, 175, 0.15)",
      "--text-primary": "#fdf2f8",
      "--text-secondary": "#fbcfe8",
      "--text-subdued": "#8a6a70",
      "--border-color": "rgba(251, 191, 36, 0.25)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #1a1416 0%, rgba(251,191,36,0.04) 50%, rgba(253,164,175,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(251,191,36,0.06) 0%, rgba(253,164,175,0.04) 50%, #1a1416 100%) !important;
                border-top: 1px solid rgba(251,191,36,0.25) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at top right, rgba(251,191,36,0.06) 0%, transparent 40%),
                    radial-gradient(ellipse at bottom left, rgba(253,164,175,0.07) 0%, transparent 40%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #fbbf24 0%, #fda4af 50%, #f9a8d4 100%) !important;
                box-shadow: 0 4px 20px rgba(251,191,36,0.3), 0 0 16px rgba(253,164,175,0.2) !important;
                color: #1a1416 !important;
                font-weight: 600 !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #fbbf24, #fda4af, #f9a8d4) !important;
                box-shadow: 0 0 10px rgba(251,191,36,0.4) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                background: linear-gradient(90deg, #fbbf24, #fda4af) !important;
                -webkit-background-clip: text !important;
                -webkit-text-fill-color: transparent !important;
                background-clip: text !important;
            }
            .icon-btn.active {
                color: #fda4af !important;
            }
        `,
  },
  "stormy-night": {
    vars: {
      "--bg-base": "#0c1117",
      "--bg-elevated": "#131922",
      "--bg-surface": "#1b232e",
      "--bg-highlight": "#27303c",
      "--bg-press": "#333d4a",
      "--accent-primary": "#60a5fa",
      "--accent-hover": "#93c5fd",
      "--accent-subtle": "rgba(96, 165, 250, 0.15)",
      "--text-primary": "#f1f5f9",
      "--text-secondary": "#93c5fd",
      "--text-subdued": "#5a6a7d",
      "--border-color": "rgba(96, 165, 250, 0.25)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0c1117 0%, rgba(96,165,250,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(96,165,250,0.07) 0%, #0c1117 100%) !important;
                border-top: 1px solid rgba(96,165,250,0.25) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    linear-gradient(180deg, rgba(15,23,42,0.5) 0%, transparent 30%, transparent 70%, rgba(15,23,42,0.7) 100%),
                    radial-gradient(ellipse at 50% 100%, rgba(96,165,250,0.06) 0%, transparent 50%);
                z-index: -1;
            }
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(1px 12px at 15% 20%, #93c5fd, transparent),
                    radial-gradient(1px 16px at 75% 35%, #60a5fa, transparent),
                    radial-gradient(1px 14px at 45% 60%, #93c5fd, transparent),
                    radial-gradient(1px 18px at 85% 75%, #60a5fa, transparent),
                    radial-gradient(1px 12px at 25% 45%, #93c5fd, transparent),
                    radial-gradient(1px 16px at 55% 85%, #60a5fa, transparent);
                background-size: 200px 200px;
                background-repeat: repeat;
                opacity: 0.5;
                z-index: -1;
                animation: rain-fall 1.2s linear infinite;
            }
            @keyframes rain-fall {
                0% { background-position: 0 0; }
                100% { background-position: 0 200px; }
            }
            .btn-primary {
                background: linear-gradient(135deg, #1e40af 0%, #60a5fa 50%, #93c5fd 100%) !important;
                box-shadow: 0 4px 20px rgba(96,165,250,0.4), 0 0 24px rgba(147,197,253,0.2) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #1e40af, #60a5fa, #93c5fd) !important;
                box-shadow: 0 0 12px rgba(96,165,250,0.5) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #93c5fd !important;
                text-shadow: 0 0 8px rgba(147,197,253,0.5) !important;
            }
            .icon-btn.active {
                color: #60a5fa !important;
            }
        `,
  },
  "toxic-acid": {
    vars: {
      "--bg-base": "#0a1008",
      "--bg-elevated": "#0f1810",
      "--bg-surface": "#152118",
      "--bg-highlight": "#1e2e22",
      "--bg-press": "#283c2c",
      "--accent-primary": "#a3e635",
      "--accent-hover": "#bef264",
      "--accent-subtle": "rgba(163, 230, 53, 0.15)",
      "--text-primary": "#f7fee7",
      "--text-secondary": "#bef264",
      "--text-subdued": "#5e7848",
      "--border-color": "rgba(163, 230, 53, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0a1008 0%, rgba(163,230,53,0.05) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(163,230,53,0.1) 0%, #0a1008 100%) !important;
                border-top: 1px solid rgba(163,230,53,0.35) !important;
                box-shadow: 0 -4px 24px rgba(163,230,53,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at 30% 20%, rgba(163,230,53,0.06) 0%, transparent 40%),
                    radial-gradient(ellipse at 70% 80%, rgba(132,204,22,0.05) 0%, transparent 40%);
                z-index: -1;
            }
            body::before {
                content: '';
                position: fixed;
                top: 0;
                left: 0;
                right: 0;
                height: 4px;
                background: repeating-linear-gradient(90deg, #a3e635 0px, #a3e635 14px, #0a1008 14px, #0a1008 28px);
                z-index: 9999;
                box-shadow: 0 0 14px rgba(163,230,53,0.6);
            }
            .btn-primary {
                background: #a3e635 !important;
                color: #0a1008 !important;
                font-weight: 800 !important;
                text-transform: uppercase !important;
                letter-spacing: 0.05em !important;
                box-shadow: 0 4px 0 #65a30d, 0 4px 20px rgba(163,230,53,0.4) !important;
            }
            .btn-primary:hover {
                transform: translateY(2px) !important;
                box-shadow: 0 2px 0 #65a30d, 0 0 16px rgba(163,230,53,0.6) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: repeating-linear-gradient(90deg, #a3e635 0px, #a3e635 6px, #65a30d 6px, #65a30d 12px) !important;
                box-shadow: 0 0 10px rgba(163,230,53,0.6) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #bef264 !important;
                text-shadow: 0 0 8px rgba(163,230,53,0.5) !important;
                font-weight: 700 !important;
            }
            .icon-btn.active {
                color: #a3e635 !important;
                text-shadow: 0 0 8px rgba(163,230,53,0.6) !important;
            }
        `,
  },
  "crimson-dusk": {
    vars: {
      "--bg-base": "#0c0608",
      "--bg-elevated": "#160a0e",
      "--bg-surface": "#1f0e15",
      "--bg-highlight": "#2d1219",
      "--bg-press": "#3a1722",
      "--accent-primary": "#e11d48",
      "--accent-hover": "#f43f5e",
      "--accent-subtle": "rgba(225, 29, 72, 0.15)",
      "--text-primary": "#fdf2f8",
      "--text-secondary": "#fda4af",
      "--text-subdued": "#7a3a4a",
      "--border-color": "rgba(159, 18, 57, 0.4)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0c0608 0%, rgba(159,18,57,0.08) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(159,18,57,0.12) 0%, rgba(127,29,29,0.05) 50%, #0c0608 100%) !important;
                border-top: 1px solid rgba(159,18,57,0.4) !important;
                box-shadow: 0 -4px 28px rgba(225,29,72,0.15) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at 50% 100%, rgba(159,18,57,0.12) 0%, transparent 60%),
                    radial-gradient(ellipse at 20% 80%, rgba(225,29,72,0.06) 0%, transparent 40%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #881337 0%, #9f1239 50%, #e11d48 100%) !important;
                color: #fff5f7 !important;
                box-shadow: 0 4px 24px rgba(225,29,72,0.5), inset 0 1px 0 rgba(255,255,255,0.1) !important;
            }
            .btn-primary:hover {
                box-shadow: 0 4px 32px rgba(225,29,72,0.7), inset 0 1px 0 rgba(255,255,255,0.2) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #881337, #9f1239, #e11d48, #f43f5e) !important;
                box-shadow: 0 0 14px rgba(225,29,72,0.6) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #f43f5e !important;
                text-shadow: 0 0 8px rgba(225,29,72,0.5) !important;
                font-weight: 600 !important;
                font-style: italic !important;
            }
            .icon-btn.active {
                color: #e11d48 !important;
                text-shadow: 0 0 6px rgba(225,29,72,0.5) !important;
            }
        `,
  },
  "mocha-espresso": {
    vars: {
      "--bg-base": "#1a100a",
      "--bg-elevated": "#221510",
      "--bg-surface": "#2b1c14",
      "--bg-highlight": "#3a261b",
      "--bg-press": "#4a3023",
      "--accent-primary": "#d97706",
      "--accent-hover": "#f59e0b",
      "--accent-subtle": "rgba(217, 119, 6, 0.15)",
      "--text-primary": "#fef3c7",
      "--text-secondary": "#fde68a",
      "--text-subdued": "#8b6c4a",
      "--border-color": "rgba(217, 119, 6, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #1a100a 0%, rgba(217,119,6,0.06) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(217,119,6,0.1) 0%, #1a100a 100%) !important;
                border-top: 1px solid rgba(217,119,6,0.25) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at 50% 0%, rgba(217,119,6,0.08) 0%, transparent 50%),
                    radial-gradient(ellipse at 0% 100%, rgba(120,53,15,0.1) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #92400e 0%, #d97706 50%, #fbbf24 100%) !important;
                color: #1a100a !important;
                font-weight: 700 !important;
                box-shadow: 0 4px 18px rgba(217,119,6,0.4) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #92400e, #c2410c, #d97706, #fbbf24) !important;
                box-shadow: 0 0 10px rgba(217,119,6,0.5) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                background: linear-gradient(90deg, #d97706, #fbbf24) !important;
                -webkit-background-clip: text !important;
                -webkit-text-fill-color: transparent !important;
                background-clip: text !important;
                font-weight: 600 !important;
            }
            .icon-btn.active {
                color: #f59e0b !important;
            }
        `,
  },
  "cobalt-royal": {
    vars: {
      "--bg-base": "#0a0f1f",
      "--bg-elevated": "#0e1525",
      "--bg-surface": "#131e30",
      "--bg-highlight": "#1c2940",
      "--bg-press": "#243450",
      "--accent-primary": "#2563eb",
      "--accent-hover": "#3b82f6",
      "--accent-subtle": "rgba(37, 99, 235, 0.15)",
      "--text-primary": "#eff6ff",
      "--text-secondary": "#93c5fd",
      "--text-subdued": "#4a6390",
      "--border-color": "rgba(37, 99, 235, 0.35)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0a0f1f 0%, rgba(37,99,235,0.08) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(37,99,235,0.1) 0%, #0a0f1f 100%) !important;
                border-top: 1px solid rgba(37,99,235,0.4) !important;
                box-shadow: 0 -4px 24px rgba(37,99,235,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at 30% 30%, rgba(37,99,235,0.08) 0%, transparent 50%),
                    radial-gradient(ellipse at 70% 70%, rgba(59,130,246,0.05) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #1e3a8a 0%, #2563eb 50%, #60a5fa 100%) !important;
                color: #fff !important;
                box-shadow: 0 4px 24px rgba(37,99,235,0.5), inset 0 1px 0 rgba(255,255,255,0.15) !important;
            }
            .btn-primary:hover {
                box-shadow: 0 4px 32px rgba(37,99,235,0.7), inset 0 1px 0 rgba(255,255,255,0.25) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #1e3a8a, #2563eb, #3b82f6, #60a5fa) !important;
                box-shadow: 0 0 14px rgba(37,99,235,0.6) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #60a5fa !important;
                text-shadow: 0 0 8px rgba(96,165,250,0.5) !important;
                font-weight: 700 !important;
            }
            .icon-btn.active {
                color: #3b82f6 !important;
                text-shadow: 0 0 6px rgba(37,99,235,0.5) !important;
            }
        `,
  },
  terracotta: {
    vars: {
      "--bg-base": "#1a1108",
      "--bg-elevated": "#241810",
      "--bg-surface": "#2e201a",
      "--bg-highlight": "#3d2a20",
      "--bg-press": "#4d3528",
      "--accent-primary": "#c2410c",
      "--accent-hover": "#ea580c",
      "--accent-subtle": "rgba(194, 65, 12, 0.15)",
      "--text-primary": "#fff7ed",
      "--text-secondary": "#fdba74",
      "--text-subdued": "#8b6f55",
      "--border-color": "rgba(194, 65, 12, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #1a1108 0%, rgba(194,65,12,0.08) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(194,65,12,0.1) 0%, #1a1108 100%) !important;
                border-top: 1px solid rgba(194,65,12,0.3) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    radial-gradient(ellipse at 80% 10%, rgba(234,88,12,0.08) 0%, transparent 50%),
                    radial-gradient(ellipse at 20% 90%, rgba(154,52,18,0.1) 0%, transparent 50%);
                z-index: -1;
            }
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: repeating-linear-gradient(45deg, transparent, transparent 60px, rgba(194,65,12,0.03) 60px, rgba(194,65,12,0.03) 61px);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #9a3412 0%, #c2410c 50%, #ea580c 100%) !important;
                color: #fff7ed !important;
                box-shadow: 0 4px 20px rgba(194,65,12,0.4) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #9a3412, #c2410c, #ea580c, #fb923c) !important;
                box-shadow: 0 0 10px rgba(194,65,12,0.5) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #fb923c !important;
                text-shadow: 0 0 8px rgba(251,146,60,0.4) !important;
            }
            .icon-btn.active {
                color: #ea580c !important;
            }
        `,
  },
  "holo-foil": {
    vars: {
      "--bg-base": "#08080f",
      "--bg-elevated": "#0c0c18",
      "--bg-surface": "#12121f",
      "--bg-highlight": "#1a1a2c",
      "--bg-press": "#22223a",
      "--accent-primary": "#c084fc",
      "--accent-hover": "#e879f9",
      "--accent-subtle": "rgba(192, 132, 252, 0.15)",
      "--text-primary": "#fafafa",
      "--text-secondary": "#e5e5e5",
      "--text-subdued": "#71717a",
      "--border-color": "rgba(192, 132, 252, 0.25)",
    },
    customStyles: `
            /* ===== HOLO FOIL ===== */
            /* Animated iridescent holographic — no static accent. */

            @keyframes holo-rotate {
                0%   { background-position: 0% 50%; }
                100% { background-position: 200% 50%; }
            }
            @keyframes holo-pulse {
                0%, 100% { opacity: 0.88; }
                50%      { opacity: 1; }
            }
            @keyframes holo-spectrum {
                0%   { filter: hue-rotate(0deg); }
                100% { filter: hue-rotate(360deg); }
            }
            @keyframes holo-chromatic {
                0%, 100% { text-shadow: -1.5px 0 #ff00ff, 1.5px 0 #00ffff, 0 0 8px rgba(192,132,252,0.5); }
                33%      { text-shadow: -1.5px 0 #ffff00, 1.5px 0 #ff00ff, 0 0 10px rgba(0,255,255,0.5); }
                66%      { text-shadow: -1.5px 0 #00ffff, 1.5px 0 #ffff00, 0 0 10px rgba(255,0,255,0.5); }
            }

            body {
                perspective: 2500px !important;
            }

            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #08080f 0%, rgba(192,132,252,0.05) 100%) !important;
            }

            .player-bar {
                background:
                    linear-gradient(180deg, rgba(192,132,252,0.08) 0%, #08080f 100%) !important;
                border-top: 1px solid rgba(192,132,252,0.2) !important;
                box-shadow: 0 -2px 24px rgba(192,132,252,0.15), inset 0 1px 0 rgba(255,255,255,0.05) !important;
            }

            /* Slow-shifting conic breath behind everything */
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    conic-gradient(from 0deg at 25% 25%, #ff00ff66, transparent 25%),
                    conic-gradient(from 180deg at 75% 75%, #00ffff66, transparent 25%),
                    conic-gradient(from 90deg at 50% 50%, transparent, rgba(255,255,0,0.05) 50%, transparent);
                animation: holo-spectrum 20s linear infinite;
                z-index: -1;
            }

            /* Interference grid — like rainbow refracted onto a screen */
            body::before {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background:
                    repeating-linear-gradient(45deg, transparent 0, transparent 6px, rgba(255,255,255,0.018) 6px, rgba(255,255,255,0.018) 7px),
                    repeating-linear-gradient(-45deg, transparent 0, transparent 6px, rgba(255,255,255,0.018) 6px, rgba(255,255,255,0.018) 7px);
                mix-blend-mode: overlay;
                opacity: 0.6;
                z-index: 9997;
            }

            /* The button cycles through the whole spectrum */
            .btn-primary {
                background: linear-gradient(135deg, #ff00ff, #00ffff, #ffff00, #ff00ff) !important;
                background-size: 300% 300% !important;
                animation: holo-rotate 3s linear infinite, holo-pulse 2.5s ease-in-out infinite !important;
                color: #08080f !important;
                font-weight: 800 !important;
                text-shadow: 1px 1px 0 rgba(255,255,255,0.3);
                border: none !important;
                box-shadow:
                    0 4px 24px rgba(192,132,252,0.5),
                    0 0 16px rgba(0,255,255,0.3),
                    inset 0 1px 0 rgba(255,255,255,0.4),
                    inset 0 -2px 4px rgba(0,0,0,0.3) !important;
            }

            /* Literal rainbow stripe progress bar — true spectrum, animated */
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg,
                    #ff00ff, #ff8000, #ffff00, #80ff00, #00ff80, #00ffff, #0080ff, #8000ff, #ff00ff
                ) !important;
                background-size: 300% 100% !important;
                animation: holo-rotate 2.5s linear infinite !important;
                box-shadow:
                    0 0 14px rgba(192,132,252,0.7),
                    0 0 28px rgba(0,255,255,0.4) !important;
            }

            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                background: linear-gradient(90deg, #ff00ff, #00ffff, #ffff00, #ff00ff) !important;
                background-size: 200% auto !important;
                -webkit-background-clip: text !important;
                -webkit-text-fill-color: transparent !important;
                background-clip: text !important;
                animation: holo-rotate 4s linear infinite !important;
                font-weight: 700 !important;
                text-shadow: none !important;
            }

            .track-item:hover {
                background: linear-gradient(90deg, rgba(255,0,255,0.1), rgba(0,255,255,0.1)) !important;
            }

            /* Chromatic-aberration text shadow that cycles through color pairs */
            .icon-btn.active {
                animation: holo-chromatic 2s ease-in-out infinite !important;
                font-weight: 700 !important;
            }

            ::selection {
                background: rgba(192, 132, 252, 0.4) !important;
                color: #fff !important;
            }
        `,
  },
  "true-black": {
    vars: {
      "--bg-base": "#000000",
      "--bg-elevated": "#000000",
      "--bg-surface": "#000000",
      "--bg-highlight": "#0c0c0c",
      "--bg-press": "#1a1a1a",
      "--accent-primary": "#ffffff",
      "--accent-hover": "#e5e5e5",
      "--accent-subtle": "rgba(255, 255, 255, 0.08)",
      "--text-primary": "#ffffff",
      "--text-secondary": "#a3a3a3",
      "--text-subdued": "#525252",
      "--border-color": "rgba(255, 255, 255, 0.08)",
    },
    customStyles: `
            /* ===== TRUE BLACK ===== */
            /* Maximum absence — OLED-friendly, zero distraction. */

            .sidebar, .nav-sidebar {
                background: #000000 !important;
                border-right: 1px solid rgba(255,255,255,0.05) !important;
            }

            .player-bar {
                background: #000000 !important;
                border-top: 1px solid rgba(255,255,255,0.07) !important;
            }

            /* No ambient overlays. No animation. No color. Pure void. */

            .btn-primary {
                background: #ffffff !important;
                color: #000000 !important;
                font-weight: 600 !important;
                border: none !important;
                letter-spacing: 0.02em !important;
                box-shadow: none !important;
                transition: opacity 0.15s ease !important;
            }
            .btn-primary:hover {
                background: #e5e5e5 !important;
                box-shadow: none !important;
            }

            .progress-bar .progress, .volume-bar .progress {
                background: #ffffff !important;
                box-shadow: none !important;
            }

            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #ffffff !important;
                font-weight: 500 !important;
                text-shadow: none !important;
                letter-spacing: 0.01em !important;
            }

            .icon-btn.active {
                color: #ffffff !important;
            }

            ::selection {
                background: #ffffff !important;
                color: #000000 !important;
            }
        `,
  },
  "neon-lime": {
    vars: {
      "--bg-base": "#0d0d00",
      "--bg-elevated": "#121206",
      "--bg-surface": "#1a1a0e",
      "--bg-highlight": "#252518",
      "--bg-press": "#303022",
      "--accent-primary": "#7FFF00",
      "--accent-hover": "#9AFF42",
      "--accent-subtle": "rgba(127, 255, 0, 0.15)",
      "--text-primary": "#f0ffe0",
      "--text-secondary": "#bfff80",
      "--text-subdued": "#8a9a4a",
      "--border-color": "rgba(127, 255, 0, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0d0d00 0%, rgba(127,255,0,0.06) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(127,255,0,0.06) 0%, rgba(127,255,0,0.03) 50%, #0d0d00 100%) !important;
                border-top: 1px solid rgba(127,255,0,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(circle at 50% 30%, rgba(127,255,0,0.05) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #5a8a00 0%, #7FFF00 50%, #9AFF42 100%) !important;
                box-shadow: 0 4px 25px rgba(127,255,0,0.4) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #7FFF00, #9AFF42) !important;
                box-shadow: 0 0 12px rgba(127,255,0,0.4) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #9AFF42 !important;
                text-shadow: 0 0 10px rgba(154,255,66,0.5) !important;
            }
            .icon-btn.active {
                color: #7FFF00 !important;
            }
        `,
  },
  "golden-hour": {
    vars: {
      "--bg-base": "#120e08",
      "--bg-elevated": "#1a150e",
      "--bg-surface": "#241d12",
      "--bg-highlight": "#302618",
      "--bg-press": "#3c3020",
      "--accent-primary": "#FFD700",
      "--accent-hover": "#FFE44D",
      "--accent-subtle": "rgba(255, 215, 0, 0.15)",
      "--text-primary": "#fff8e0",
      "--text-secondary": "#ffec8b",
      "--text-subdued": "#a09060",
      "--border-color": "rgba(255, 215, 0, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #120e08 0%, rgba(255,215,0,0.06) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(255,215,0,0.06) 0%, rgba(255,215,0,0.03) 50%, #120e08 100%) !important;
                border-top: 1px solid rgba(255,215,0,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(circle at 20% 80%, rgba(255,215,0,0.06) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #8a7200 0%, #FFD700 50%, #FFE44D 100%) !important;
                box-shadow: 0 4px 20px rgba(255,215,0,0.3) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #FFD700, #FFE44D) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #FFE44D !important;
            }
            .icon-btn.active {
                color: #FFD700 !important;
            }
        `,
  },
  "indigo-night": {
    vars: {
      "--bg-base": "#08041a",
      "--bg-elevated": "#0e0725",
      "--bg-surface": "#160c30",
      "--bg-highlight": "#201540",
      "--bg-press": "#2a1e50",
      "--accent-primary": "#6A0DAD",
      "--accent-hover": "#8B2FC9",
      "--accent-subtle": "rgba(106, 13, 173, 0.15)",
      "--text-primary": "#f0e6ff",
      "--text-secondary": "#c9a0f0",
      "--text-subdued": "#9585a0",
      "--border-color": "rgba(106, 13, 173, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #08041a 0%, rgba(106,13,173,0.07) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(106,13,173,0.08) 0%, rgba(106,13,173,0.03) 50%, #08041a 100%) !important;
                border-top: 1px solid rgba(106,13,173,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(circle at 50% 40%, rgba(106,13,173,0.06) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #4a0a7a 0%, #6A0DAD 50%, #8B2FC9 100%) !important;
                box-shadow: 0 4px 25px rgba(106,13,173,0.35) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #6A0DAD, #8B2FC9) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #8B2FC9 !important;
                text-shadow: 0 0 8px rgba(139,47,201,0.4) !important;
            }
            .icon-btn.active {
                color: #6A0DAD !important;
            }
        `,
  },
  "coral-reef": {
    vars: {
      "--bg-base": "#140a0a",
      "--bg-elevated": "#1c0f0e",
      "--bg-surface": "#261612",
      "--bg-highlight": "#321e18",
      "--bg-press": "#3e261e",
      "--accent-primary": "#FF7F50",
      "--accent-hover": "#FFA070",
      "--accent-subtle": "rgba(255, 127, 80, 0.15)",
      "--text-primary": "#fff0ea",
      "--text-secondary": "#ffb89a",
      "--text-subdued": "#9a8070",
      "--border-color": "rgba(255, 127, 80, 0.3)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #140a0a 0%, rgba(255,127,80,0.06) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(255,127,80,0.06) 0%, rgba(255,127,80,0.03) 50%, #140a0a 100%) !important;
                border-top: 1px solid rgba(255,127,80,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(circle at 80% 80%, rgba(255,127,80,0.06) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #cc5a30 0%, #FF7F50 50%, #FFA070 100%) !important;
                box-shadow: 0 4px 20px rgba(255,127,80,0.3) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #FF7F50, #FFA070) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #FFA070 !important;
            }
            .icon-btn.active {
                color: #FF7F50 !important;
            }
        `,
  },
  "frosted-amethyst": {
    vars: {
      "--bg-base": "#0a0a14",
      "--bg-elevated": "#10102e",
      "--bg-surface": "#1a1a3e",
      "--bg-highlight": "#25254e",
      "--bg-press": "#30305e",
      "--accent-primary": "#9966CC",
      "--accent-hover": "#B388E0",
      "--accent-subtle": "rgba(153, 102, 204, 0.15)",
      "--text-primary": "#f0e6ff",
      "--text-secondary": "#c4a0e8",
      "--text-subdued": "#958aaa",
      "--border-color": "rgba(153, 102, 204, 0.25)",
    },
    customStyles: `
            .sidebar, .nav-sidebar {
                background: linear-gradient(180deg, #0a0a14 0%, rgba(153,102,204,0.07) 100%) !important;
            }
            .player-bar {
                background: linear-gradient(180deg, rgba(153,102,204,0.07) 0%, rgba(153,102,204,0.03) 50%, #0a0a14 100%) !important;
                border-top: 1px solid rgba(153,102,204,0.2) !important;
            }
            body::after {
                content: '';
                position: fixed;
                inset: 0;
                pointer-events: none;
                background: radial-gradient(circle at 30% 30%, rgba(153,102,204,0.06) 0%, transparent 50%);
                z-index: -1;
            }
            .btn-primary {
                background: linear-gradient(135deg, #704aaa 0%, #9966CC 50%, #B388E0 100%) !important;
                box-shadow: 0 4px 20px rgba(153,102,204,0.3) !important;
            }
            .progress-bar .progress, .volume-bar .progress {
                background: linear-gradient(90deg, #9966CC, #B388E0) !important;
            }
            .track-item.playing .track-title,
            .queue-track.current .track-title,
            .track-row.playing .track-title {
                color: #B388E0 !important;
            }
            .icon-btn.active {
                color: #9966CC !important;
            }
        `,
  },
};

const THEME_STORAGE_KEY = "rlist_theme";

// Default theme state
const defaultTheme: ThemeState = {
  mode: "dark",
  accentColor: "#1DB954",
  customAccentColors: [],
};

// Load theme from localStorage
function loadTheme(): ThemeState {
  if (typeof window === "undefined") return defaultTheme;

  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored) {
      return { ...defaultTheme, ...JSON.parse(stored) };
    }
  } catch (error) {
    console.error("[Theme] Failed to load:", error);
  }

  return defaultTheme;
}

// Save theme to localStorage
function saveTheme(state: ThemeState): void {
  if (typeof window === "undefined") return;

  try {
    localStorage.setItem(THEME_STORAGE_KEY, JSON.stringify(state));
  } catch (error) {
    console.error("[Theme] Failed to save:", error);
  }
}

// Create theme store
function createThemeStore() {
  // Start with default theme - actual theme is loaded in initialize() when browser is ready
  const { subscribe, set, update } = writable<ThemeState>(defaultTheme);

  return {
    subscribe,

    setMode(mode: ThemeMode) {
      update((state) => {
        const newState = { ...state, mode };
        saveTheme(newState);
        applyTheme(newState);
        return newState;
      });
    },

    setAccentColor(color: string) {
      update((state) => {
        const newState = { ...state, accentColor: color };
        saveTheme(newState);
        applyTheme(newState);
        return newState;
      });
    },

    addCustomColor(color: string) {
      update((state) => {
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
    },
  };
}

export const theme = createThemeStore();

// Lighten a color for hover state
function lightenColor(hex: string, percent: number): string {
  const num = parseInt(hex.replace("#", ""), 16);
  const amt = Math.round(2.55 * percent);
  const R = Math.min(255, (num >> 16) + amt);
  const G = Math.min(255, ((num >> 8) & 0x00ff) + amt);
  const B = Math.min(255, (num & 0x0000ff) + amt);
  return `#${((1 << 24) | (R << 16) | (G << 8) | B).toString(16).slice(1)}`;
}

// Darken a color
function darkenColor(hex: string, percent: number): string {
  const num = parseInt(hex.replace("#", ""), 16);
  const amt = Math.round(2.55 * percent);
  const R = Math.max(0, (num >> 16) - amt);
  const G = Math.max(0, ((num >> 8) & 0x00ff) - amt);
  const B = Math.max(0, (num & 0x0000ff) - amt);
  return `#${((1 << 24) | (R << 16) | (G << 8) | B).toString(16).slice(1)}`;
}

// Convert hex to RGB string (r, g, b)
function hexToRgb(hex: string): string {
  const num = parseInt(hex.replace("#", ""), 16);
  const R = num >> 16;
  const G = (num >> 8) & 0x00ff;
  const B = num & 0x0000ff;
  return `${R}, ${G}, ${B}`;
}

// ID for the injected custom theme stylesheet
const CUSTOM_THEME_STYLE_ID = "audion-custom-theme-styles";

function removeCustomThemeStyles(): void {
  const existing = document.getElementById(CUSTOM_THEME_STYLE_ID);
  if (existing) existing.remove();
}

function injectCustomThemeStyles(css: string): void {
  removeCustomThemeStyles();
  const style = document.createElement("style");
  style.id = CUSTOM_THEME_STYLE_ID;
  style.textContent = css;
  document.head.appendChild(style);
}

// Apply theme to CSS variables
export function applyTheme(state: ThemeState): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;

  // Handle preset themes (data-driven)
  const definition = themeDefinitions[state.mode];
  if (definition) {
    for (const [key, value] of Object.entries(definition.vars)) {
      root.style.setProperty(key, value);
    }
    // Set accent RGB for rgba() usage
    const accent = definition.vars["--accent-primary"];
    if (accent && accent.startsWith("#")) {
      root.style.setProperty("--accent-primary-rgb", hexToRgb(accent));
    }
    root.setAttribute("data-theme", state.mode);
    if (definition.customStyles) {
      injectCustomThemeStyles(definition.customStyles);
    } else {
      removeCustomThemeStyles();
    }
    return;
  }

  // Standard themes (dark/light/system) — remove any custom preset styles
  removeCustomThemeStyles();

  const isDark =
    state.mode === "dark" ||
    (state.mode === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);

  // Apply accent colors
  root.style.setProperty("--accent-primary", state.accentColor);
  root.style.setProperty("--accent-primary-rgb", hexToRgb(state.accentColor));
  root.style.setProperty("--accent-hover", lightenColor(state.accentColor, 15));
  root.style.setProperty("--accent-subtle", state.accentColor + "20");

  if (isDark) {
    // Dark theme
    root.style.setProperty("--bg-base", "#121212");
    root.style.setProperty("--bg-elevated", "#181818");
    root.style.setProperty("--bg-surface", "#282828");
    root.style.setProperty("--bg-highlight", "#3e3e3e");
    root.style.setProperty("--bg-press", "#535353");
    root.style.setProperty("--text-primary", "#ffffff");
    root.style.setProperty("--text-secondary", "#b3b3b3");
    root.style.setProperty("--text-subdued", "#6a6a6a");
    root.style.setProperty("--border-color", "#404040");
  } else {
    // Light theme
    root.style.setProperty("--bg-base", "#f5f5f5");
    root.style.setProperty("--bg-elevated", "#ffffff");
    root.style.setProperty("--bg-surface", "#e8e8e8");
    root.style.setProperty("--bg-highlight", "#d4d4d4");
    root.style.setProperty("--bg-press", "#c0c0c0");
    root.style.setProperty("--text-primary", "#121212");
    root.style.setProperty("--text-secondary", "#535353");
    root.style.setProperty("--text-subdued", "#8a8a8a");
    root.style.setProperty("--border-color", "#d0d0d0");
  }

  // Add theme attribute to root for CSS selectors
  root.setAttribute("data-theme", isDark ? "dark" : "light");
}

// Derived store for current theme mode
export const isDarkMode = derived(theme, ($theme) => {
  // All preset themes are dark-based
  if ($theme.mode in themeDefinitions) return true;
  if ($theme.mode === "system") {
    if (typeof window === "undefined") return true;
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  }
  return $theme.mode === "dark";
});
