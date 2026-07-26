/** @type {import('tailwindcss').Config} */

// Colours are exposed as CSS custom properties (see src/styles/tokens.css), not
// as literal hex values, so a single `data-theme` / `data-accent` swap on <html>
// restyles every utility class in the app. That is what lets components name a
// surface ("bg-panel") instead of branching on the active theme in JS.
//
// `darkMode` is deliberately absent: there is no `dark:` variant in this design
// system. Both themes are first-class and resolve through the same token names,
// so a `dark:` prefix would be a second, competing mechanism.
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        void: 'var(--void)',
        panel: 'var(--panel)',
        raised: 'var(--raised)',
        'raised-2': 'var(--raised-2)',
        line: 'var(--line)',
        'line-strong': 'var(--line-strong)',

        ink: 'var(--text)',
        dim: 'var(--text-dim)',
        mute: 'var(--text-mute)',

        accent: {
          DEFAULT: 'var(--accent-solid)',
          hover: 'var(--accent-hover)',
          soft: 'var(--accent-soft)',
          line: 'var(--accent-line)',
          text: 'var(--accent-text)',
          on: 'var(--accent-on)',
        },

        // The fixed four-colour signature. Unlike `accent`, these never follow
        // the user's accent choice.
        sfc: {
          red: 'var(--sfc-red)',
          yellow: 'var(--sfc-yellow)',
          green: 'var(--sfc-green)',
          blue: 'var(--sfc-blue)',
        },

        danger: {
          DEFAULT: 'var(--h-red-solid)',
          soft: 'var(--h-red-soft)',
          line: 'var(--h-red-line)',
          text: 'var(--h-red-text)',
        },
        success: {
          DEFAULT: 'var(--h-green-solid)',
          soft: 'var(--h-green-soft)',
          line: 'var(--h-green-line)',
          text: 'var(--h-green-text)',
        },
        warn: {
          DEFAULT: 'var(--h-yellow-solid)',
          soft: 'var(--h-yellow-soft)',
          line: 'var(--h-yellow-line)',
          text: 'var(--h-yellow-text)',
        },

        // Kept as an alias of the accent so any `bg-primary-600` still in the
        // tree (the cheats manager and the welcome wizard both carry some)
        // resolves to the live accent rather than to a hardcoded indigo that no
        // longer belongs to the palette.
        primary: {
          400: 'var(--accent-text)',
          500: 'var(--accent-solid)',
          600: 'var(--accent-solid)',
          700: 'var(--accent-hover)',
          800: 'var(--accent-solid)',
        },
      },
      fontFamily: {
        sans: ['var(--font-sans)'],
        display: ['var(--font-display)'],
        mono: ['var(--font-mono)'],
      },
      borderRadius: {
        sm: 'var(--r-sm)',
        md: 'var(--r-md)',
        lg: 'var(--r-lg)',
        xl: 'var(--r-xl)',
      },
      boxShadow: {
        sm: 'var(--shadow-sm)',
        md: 'var(--shadow-md)',
        lg: 'var(--shadow-lg)',
      },
      fontWeight: {
        // Segoe UI Variable interpolates, so a 550 mid-weight is available and
        // reads better than 600 for UI labels at small sizes.
        medium: '500',
        semibold: '550',
      },
    },
  },
  plugins: [],
};
