/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ['class', '.app-dark'],
  content: ['./index.html', './src/**/*.{vue,ts,tsx,js,jsx}'],
  theme: {
    extend: {
      colors: {
        industrial: {
          bg: '#09090b',
          panel: '#18181b',
          border: '#27272a',
          cyan: '#22d3ee',
          amber: '#f59e0b',
          danger: '#dc2626',
        },
      },
      borderRadius: {
        none: '0px',
        sm: '2px',
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Cascadia Code', 'Fira Code', 'ui-monospace', 'monospace'],
      },
      boxShadow: {
        none: 'none',
      },
    },
  },
  plugins: [],
}
