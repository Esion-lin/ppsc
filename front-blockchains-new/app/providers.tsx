'use client'

import { ThemeProvider } from 'next-themes'
import type { ReactNode } from 'react'

// 默认深色、不跟随系统、localStorage key = meta_theme
export function Providers({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider
      attribute="data-theme"
      defaultTheme="dark"
      enableSystem={false}
      storageKey="meta_theme"
    >
      {children}
    </ThemeProvider>
  )
}
