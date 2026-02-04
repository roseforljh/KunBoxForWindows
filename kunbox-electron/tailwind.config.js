/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/renderer/**/*.{html,tsx,ts}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        bg: {
          primary: 'var(--bg-primary)',
          secondary: 'var(--bg-secondary)',
          tertiary: 'var(--bg-tertiary)',
          card: 'var(--bg-card)',
          elevated: 'var(--bg-elevated)',
          hover: 'var(--bg-hover)',
          deep: 'var(--bg-primary)',
          base: 'var(--bg-primary)'
        },
        accent: {
          DEFAULT: '#14b8a6',
          primary: '#14b8a6',
          secondary: '#0d9488',
          tertiary: '#0f766e',
          muted: 'var(--accent-muted)',
          glow: 'var(--accent-glow)',
          foreground: 'hsl(var(--accent-foreground))'
        },
        text: {
          primary: 'var(--text-primary)',
          secondary: 'var(--text-secondary)',
          muted: 'var(--text-muted)',
          faint: 'var(--text-faint)'
        },
        border: {
          primary: 'var(--border-primary)',
          secondary: 'var(--border-secondary)',
          accent: 'var(--border-accent)',
          subtle: 'rgba(255, 255, 255, 0.1)',
          hover: 'rgba(255, 255, 255, 0.2)'
        },
        status: {
          success: '#10b981',
          warning: '#f59e0b',
          error: '#ef4444',
          info: '#3b82f6'
        },
        neon: {
          blue: '#3b82f6',
          purple: '#8b5cf6',
          pink: '#ec4899',
          teal: '#14b8a6',
          green: '#10b981',
          amber: '#f59e0b',
          red: '#ef4444'
        },
        calm: {
          teal: {
            50: '#f0fdfa',
            100: '#ccfbf1',
            200: '#99f6e4',
            300: '#5eead4',
            400: '#2dd4bf',
            500: '#14b8a6',
            600: '#0d9488',
            700: '#0f766e',
            800: '#115e59',
            900: '#134e4a'
          },
          sage: {
            50: '#f6f7f6',
            100: '#e3e5e3',
            200: '#c6ccc6',
            300: '#a1aba1',
            400: '#7a867a',
            500: '#5f6b5f',
            600: '#4b554b',
            700: '#3e463e',
            800: '#343a34',
            900: '#2d312d'
          }
        },
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))'
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))'
        },
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))'
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))'
        },
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))'
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))'
        },
        ring: 'hsl(var(--ring))',
        input: 'hsl(var(--input))'
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
        xl: 'var(--radius-xl)'
      },
      boxShadow: {
        'soft-sm': '0 2px 8px rgba(0, 0, 0, 0.15)',
        'soft-md': '0 4px 16px rgba(0, 0, 0, 0.2)',
        'soft-lg': '0 8px 32px rgba(0, 0, 0, 0.25)',
        'soft-xl': '0 16px 48px rgba(0, 0, 0, 0.3)',
        'glow-accent': 'var(--shadow-glow)',
        'glow-blue': '0 0 20px rgba(59, 130, 246, 0.3)',
        'glow-green': '0 0 20px rgba(16, 185, 129, 0.3)',
        'glow-red': '0 0 20px rgba(239, 68, 68, 0.3)',
        'glow-teal': '0 0 20px rgba(20, 184, 166, 0.3)',
        'inner-glow': 'inset 0 0 20px rgba(255, 255, 255, 0.05)'
      },
      fontFamily: {
        sans: ['-apple-system', 'BlinkMacSystemFont', '"SF Pro Display"', '"SF Pro Text"', '"Helvetica Neue"', 'sans-serif'],
        mono: ['"JetBrains Mono"', '"SF Mono"', '"Fira Code"', 'monospace']
      },
      backdropBlur: {
        xs: '2px',
        glass: '12px'
      },
      animation: {
        'bokeh-float': 'bokeh-float 20s ease-in-out infinite',
        'pulse-accent': 'pulse-accent 2s ease-in-out infinite',
        'fade-in': 'fade-in 0.2s ease-out',
        'slide-up': 'slide-up 0.2s ease-out',
        'slide-down': 'slide-down 0.2s ease-out'
      },
      keyframes: {
        'bokeh-float': {
          '0%, 100%': { transform: 'translate(0, 0) scale(1)' },
          '25%': { transform: 'translate(30px, -30px) scale(1.05)' },
          '50%': { transform: 'translate(-20px, 20px) scale(0.95)' },
          '75%': { transform: 'translate(20px, 10px) scale(1.02)' }
        },
        'pulse-accent': {
          '0%, 100%': { boxShadow: '0 0 0 0 var(--accent-muted)' },
          '50%': { boxShadow: '0 0 0 8px transparent' }
        },
        'fade-in': {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' }
        },
        'slide-up': {
          '0%': { opacity: '0', transform: 'translateY(10px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' }
        },
        'slide-down': {
          '0%': { opacity: '0', transform: 'translateY(-10px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' }
        }
      },
      transitionTimingFunction: {
        'bounce-in': 'cubic-bezier(0.68, -0.55, 0.265, 1.55)'
      }
    }
  },
  plugins: []
}
