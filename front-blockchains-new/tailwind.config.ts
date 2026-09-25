import type { Config } from 'tailwindcss'
import colors from 'tailwindcss/colors'
import defaultTheme from 'tailwindcss/defaultTheme'
import typography from '@tailwindcss/typography'

// 金棕色 #bd7c40 
export default {
  content: [
    './app/**/*.{js,ts,jsx,tsx,mdx}',
    './components/**/*.{js,ts,jsx,tsx,mdx}',
    './lib/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    ringColor: {
      ...colors,
      DEFAULT: '#bd7c40',
    },
    colors: {
      ms: {
        200: '#d6b58d80',
        300: '#d6b58d',
        400: '#c99965',
        500: '#bd7c40',
        600: '#b78c5d',
        700: '#a46c39',
        800: '#604328',
        900: '#281E16',
      },
      success: '#22C55E',
      error: '#B91C1C',
      orange_self: '#FB923C',
      ...colors,
    },
    extend: {
      fontFamily: {
        sans: ['var(--font-poppins)', 'Poppins', ...defaultTheme.fontFamily.sans],
        inter: ['var(--font-inter)', 'Inter', ...defaultTheme.fontFamily.sans],
        interBold: ['var(--font-inter)', 'Inter', ...defaultTheme.fontFamily.sans],
      },
      zIndex: {
        100: '100',
      },
      boxShadow: {
        lift: 'var(--lift-shadow)',
      },
    },
  },
  plugins: [typography],
} satisfies Config
