/** @type {import('tailwindcss').Config} */
const accentPalette = {
  50:  'rgb(var(--color-primary-50)  / <alpha-value>)',
  100: 'rgb(var(--color-primary-100) / <alpha-value>)',
  200: 'rgb(var(--color-primary-200) / <alpha-value>)',
  300: 'rgb(var(--color-primary-300) / <alpha-value>)',
  400: 'rgb(var(--color-primary-400) / <alpha-value>)',
  500: 'rgb(var(--color-primary-500) / <alpha-value>)',
  600: 'rgb(var(--color-primary-600) / <alpha-value>)',
  700: 'rgb(var(--color-primary-700) / <alpha-value>)',
  800: 'rgb(var(--color-primary-800) / <alpha-value>)',
  900: 'rgb(var(--color-primary-900) / <alpha-value>)',
  950: 'rgb(var(--color-primary-950) / <alpha-value>)',
};

export default {
  content: [
    "./index.html",
    "./src/**/*.{svelte,js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        primary: accentPalette,
        // Backward-compatible alias so existing blue-* utilities follow accent settings.
        blue: accentPalette
      },
      borderRadius: {
        none: '0',
        sm: '0.0625rem',    // 1px (was 2px)
        DEFAULT: '0.125rem', // 2px (was 4px)
        md: '0.1875rem',    // 3px (was 6px)
        lg: '0.25rem',      // 4px (was 8px)
        xl: '0.375rem',     // 6px (was 12px)
        '2xl': '0.5rem',    // 8px (was 16px)
        '3xl': '0.75rem',   // 12px (was 24px)
        full: '9999px',
      },
    },
  },
  plugins: [],
}
