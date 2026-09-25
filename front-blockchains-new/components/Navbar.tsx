'use client'

import Link from 'next/link'
import { useState } from 'react'
import { navItems, siteTitle, type NavItem } from '@/lib/content'
import ThemeSwitcher from './ThemeSwitcher'

export default function Navbar() {
  const [open, setOpen] = useState(false)

  return (
    <header className="sticky top-0 z-50 bg-[var(--primary-background)]/90 backdrop-blur">
      <nav className="mx-auto flex min-h-[72px] max-w-7xl items-center justify-between px-5 md:px-6">
        <Link
          href="/#home"
          className="group flex items-center gap-3 rounded-full py-2 pr-3 font-inter text-base font-semibold md:text-lg"
        >
          <span className="max-w-[5rem] truncate sm:max-w-[13rem] xl:max-w-none">{siteTitle}</span>
        </Link>

        <div className="hidden items-center gap-1 lg:flex">
          {navItems.map((item) => (
            <DesktopItem key={item.label} item={item} />
          ))}
          <Link href="/wallet" className="btn-primary ml-2 whitespace-nowrap">尝试PPSC</Link>
          <ThemeSwitcher />
        </div>

        <div className="flex items-center gap-1 lg:hidden">
          <Link href="/wallet" onClick={() => setOpen(false)} className="inline-flex min-h-11 items-center justify-center whitespace-nowrap rounded-full bg-[var(--default-color)] px-3 text-xs font-semibold text-white hover:bg-[var(--default-color-hover)] hover:text-white">尝试PPSC</Link>
          <ThemeSwitcher />
          <button
            aria-label={open ? '关闭菜单' : '打开菜单'}
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
            className="inline-flex h-11 w-11 items-center justify-center rounded-full border border-[var(--default-border-color)] bg-[var(--glass-background)] text-[var(--default-text-color)] transition hover:border-[var(--default-color)] hover:text-[var(--default-color)]"
          >
            <svg
              className="h-6 w-6"
              fill="none"
              viewBox="0 0 24 24"
              strokeWidth={1.5}
              stroke="currentColor"
            >
              {open ? (
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
              ) : (
                <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" />
              )}
            </svg>
          </button>
        </div>
      </nav>

      {open && (
        <div className="border-t border-[var(--default-border-color)] bg-[var(--primary-background)] px-5 py-4 shadow-lift backdrop-blur-xl lg:hidden">
          <MobileItems items={navItems} depth={0} onSelect={() => setOpen(false)} />
        </div>
      )}
    </header>
  )
}

/* ------------------------------ 桌面端 ------------------------------ */

function DesktopItem({ item }: { item: NavItem }) {
  if (!item.children) {
    return (
      <Link
        href={item.href ?? '#'}
        className="inline-flex min-h-11 items-center rounded-full px-4 text-sm font-medium transition hover:bg-[var(--list-item-hover-color)] hover:text-[var(--default-color)]"
      >
        {item.label}
      </Link>
    )
  }

  return (
    <div className="group relative">
      <button className="inline-flex min-h-11 items-center gap-1 rounded-full px-4 text-sm font-medium transition hover:bg-[var(--list-item-hover-color)] hover:text-[var(--default-color)] group-focus-within:bg-[var(--list-item-hover-color)]">
        {item.label}
        <svg className="h-3.5 w-3.5" viewBox="0 0 20 20" fill="currentColor">
          <path
            fillRule="evenodd"
            d="M5.23 7.21a.75.75 0 0 1 1.06.02L10 11.168l3.71-3.938a.75.75 0 1 1 1.08 1.04l-4.25 4.5a.75.75 0 0 1-1.08 0l-4.25-4.5a.75.75 0 0 1 .02-1.06Z"
            clipRule="evenodd"
          />
        </svg>
      </button>

      <div className="invisible absolute left-0 top-full pt-3 opacity-0 transition-all duration-200 group-hover:visible group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:visible group-focus-within:translate-y-0 group-focus-within:opacity-100">
        <div className="min-w-[248px] rounded-lg border border-[var(--default-border-color)] bg-[var(--dropdown-bgc)] p-2 shadow-lift">
          <div className="mb-1 border-b border-[var(--default-border-color)] px-3 pb-2 pt-1 text-xs font-semibold text-[var(--desc-color)]">
            业务导航
          </div>
          {item.children.map((child, index) => (
            <ChildItem key={`${item.label}-${child.label}-${index}`} item={child} />
          ))}
        </div>
      </div>
    </div>
  )
}

function ChildItem({ item }: { item: NavItem }) {
  const [open, setOpen] = useState(false)

  if (!item.children) {
    return (
      <Link
        href={item.href ?? '#'}
        className="block min-h-11 rounded-md px-3 py-3 text-sm transition hover:bg-[var(--dropdown-hover)] hover:text-[var(--default-color)]"
      >
        {item.label}
      </Link>
    )
  }

  // 二级子菜单：点击后在下方展开（手风琴式）
  return (
    <div>
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex min-h-11 w-full items-center justify-between rounded-md px-3 text-sm transition hover:bg-[var(--dropdown-hover)] hover:text-[var(--default-color)]"
      >
        {item.label}
        <svg
          className={`h-3.5 w-3.5 transition-transform ${open ? 'rotate-180' : ''}`}
          viewBox="0 0 20 20"
          fill="currentColor"
        >
          <path
            fillRule="evenodd"
            d="M5.23 7.21a.75.75 0 0 1 1.06.02L10 11.168l3.71-3.938a.75.75 0 1 1 1.08 1.04l-4.25 4.5a.75.75 0 0 1-1.08 0l-4.25-4.5a.75.75 0 0 1 .02-1.06Z"
            clipRule="evenodd"
          />
        </svg>
      </button>

      {open && (
        <div className="pb-1">
          {item.children.map((gc, index) => (
            <Link
              key={`${item.label}-${gc.label}-${index}`}
              href={gc.href ?? '#'}
              className="block min-h-11 rounded-md py-3 pl-8 pr-4 text-sm transition hover:bg-[var(--dropdown-hover)] hover:text-[var(--default-color)]"
            >
              {gc.label}
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}

/* ------------------------------ 移动端 ------------------------------ */

function MobileItems({
  items,
  depth,
  onSelect,
}: {
  items: NavItem[]
  depth: number
  onSelect: () => void
}) {
  return (
    <ul className={depth > 0 ? 'ml-4 border-l border-[var(--default-border-color)] pl-3' : 'space-y-1'}>
      {items.map((item, index) => (
        <li key={`${depth}-${item.label}-${index}`}>
          {item.href ? (
            <Link
              href={item.href}
              onClick={onSelect}
              className="block min-h-11 rounded-lg px-3 py-3 text-sm font-medium hover:bg-[var(--list-item-hover-color)] hover:text-[var(--default-color)]"
            >
              {item.label}
            </Link>
          ) : (
            <span className="block px-3 py-3 text-sm font-semibold text-[var(--default-text-color)]">
              {item.label}
            </span>
          )}
          {item.children && (
            <MobileItems items={item.children} depth={depth + 1} onSelect={onSelect} />
          )}
        </li>
      ))}
    </ul>
  )
}
