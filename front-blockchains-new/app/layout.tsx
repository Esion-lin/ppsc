import type { Metadata } from 'next'
import { Providers } from './providers'
import { siteTitle, siteSubtitle } from '@/lib/content'
import './globals.css'

export const metadata: Metadata = {
  title: siteTitle,
  description: siteSubtitle,
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html
      lang="zh-CN"
      suppressHydrationWarning
    >
      <body className="antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  )
}
