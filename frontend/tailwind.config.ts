import type { Config } from 'tailwindcss'

export default <Partial<Config>>{
  content: [
    './components/**/*.{vue,js,ts}',
    './layouts/**/*.vue',
    './pages/**/*.vue',
    './app.vue'
  ],
  darkMode: 'class',
  theme: {
    extend: {
      fontFamily: {
        sans: [
          '-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'Noto Sans', 'Helvetica',
          'Arial', 'sans-serif', 'Apple Color Emoji', 'Segoe UI Emoji'
        ],
        mono: [
          'ui-monospace', 'SFMono-Regular', 'SF Mono', 'Menlo', 'Consolas', 'Liberation Mono', 'monospace'
        ]
      },
      colors: {
        canvas: {
          DEFAULT: '#ffffff',
          dark: '#0d1117',
          subtle: '#f6f8fa',
          'subtle-dark': '#161b22'
        },
        border: {
          DEFAULT: '#d0d7de',
          dark: '#30363d',
          muted: '#d8dee4',
          'muted-dark': '#21262d'
        },
        fg: {
          DEFAULT: '#1f2328',
          dark: '#e6edf3',
          muted: '#59636e',
          'muted-dark': '#8b949e'
        },
        accent: {
          DEFAULT: '#0969da',
          dark: '#2f81f7',
          emphasis: '#0550ae'
        },
        success: {
          DEFAULT: '#1a7f37',
          dark: '#3fb950',
          emphasis: '#2da44e'
        },
        danger: {
          DEFAULT: '#cf222e',
          dark: '#f85149',
          emphasis: '#da3633'
        },
        done: {
          DEFAULT: '#8250df',
          dark: '#a371f7'
        },
        attention: {
          DEFAULT: '#9a6700',
          dark: '#d29922'
        }
      }
    }
  },
  plugins: []
}
