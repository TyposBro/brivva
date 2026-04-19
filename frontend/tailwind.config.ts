import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        primary: "#b4c5ff",
        "primary-container": "#2563eb",
        secondary: "#ddb8ff",
        "secondary-container": "#7c03d3",
        background: "#131313",
        surface: "#131313",
        "surface-dim": "#131313",
        "surface-bright": "#3a3939",
        "surface-container-lowest": "#0e0e0e",
        "surface-container-low": "#1c1b1b",
        "surface-container": "#201f1f",
        "surface-container-high": "#2a2a2a",
        "surface-container-highest": "#353534",
        "surface-variant": "#353534",
        "on-surface": "#e5e2e1",
        "on-surface-variant": "#c3c6d7",
        "on-background": "#e5e2e1",
        "on-primary": "#002a78",
        "on-primary-container": "#eeefff",
        "on-secondary": "#490080",
        "on-secondary-container": "#dfbcff",
        outline: "#8d90a0",
        "outline-variant": "#434655",
        error: "#ffb4ab",
        "error-container": "#93000a",
        "on-error": "#690005",
        "on-error-container": "#ffdad6",
        success: "#34d399",
        // Amber-ish warning — distinct from error (red) and success (green).
        // Used for advisory notes that should catch the eye without signalling
        // a broken state (e.g. the Grip one-shot-key reminder).
        warning: "#f59e0b",
      },
      fontFamily: {
        headline: ["Space Grotesk", "sans-serif"],
        body: ["Inter", "sans-serif"],
        label: ["Inter", "sans-serif"],
        mono: ["ui-monospace", "SFMono-Regular", "monospace"],
      },
      borderRadius: {
        DEFAULT: "0.125rem",
        lg: "0.25rem",
        xl: "0.5rem",
        full: "0.75rem",
      },
    },
  },
  plugins: [],
} satisfies Config;
