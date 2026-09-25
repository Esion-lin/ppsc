'use client'

import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useState } from 'react'
import { slides } from '@/lib/content'

const variants = {
  enter: (dir: number) => ({ x: dir > 0 ? '8%' : '-8%', opacity: 0 }),
  center: { x: 0, opacity: 1 },
  exit: (dir: number) => ({ x: dir > 0 ? '-8%' : '8%', opacity: 0 }),
}

const placeholderBg =
  'linear-gradient(135deg, var(--hero-from) 0%, var(--hero-via) 52%, var(--hero-to) 100%)'

export default function HeroCarousel() {
  const [[index, direction], setIndex] = useState<[number, number]>([0, 0])
  const [hoverPaused, setHoverPaused] = useState(false)
  const paused = hoverPaused

  const paginate = useCallback((dir: number) => {
    setIndex(([i]) => [(i + dir + slides.length) % slides.length, dir])
  }, [])

  const goTo = (i: number) =>
    setIndex(([cur]) => [i, i === cur ? 0 : i > cur ? 1 : -1])

  useEffect(() => {
    if (paused) return

    const timer = setInterval(() => paginate(1), 6200)
    return () => clearInterval(timer)
  }, [paginate, paused])

  const slide = slides[index]

  return (
    <section
      id="home"
      className="relative isolate min-h-[620px] overflow-hidden"
      onMouseEnter={() => setHoverPaused(true)}
      onMouseLeave={() => setHoverPaused(false)}
    >
      <AnimatePresence initial={false} custom={direction} mode="wait">
        <motion.div
          key={index}
          custom={direction}
          variants={variants}
          initial="enter"
          animate="center"
          exit="exit"
          transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
          className="absolute inset-0"
        >
          <div
            className="absolute inset-0 bg-cover bg-center"
            style={{
              backgroundImage: slide.image
                ? `linear-gradient(115deg, rgb(8 16 20 / 0.88), rgb(8 16 20 / 0.48)), url(${slide.image})`
                : placeholderBg,
            }}
          />
        </motion.div>
      </AnimatePresence>

      <div className="absolute inset-0 bg-[linear-gradient(90deg,rgb(0_0_0/0.72),rgb(0_0_0/0.36),rgb(0_0_0/0.54))]" />
      <div className="absolute inset-x-0 bottom-0 h-28 bg-gradient-to-t from-[var(--primary-background)] to-transparent" />

      <div className="relative mx-auto grid min-h-[620px] max-w-7xl items-center gap-10 px-5 py-20 md:px-6 lg:grid-cols-[1.05fr_0.95fr]">
        <div className="max-w-3xl text-white">
          <motion.h1
            key={`${index}-title`}
            initial={{ opacity: 0, y: 18 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45 }}
            className="font-inter text-4xl font-bold leading-tight md:text-6xl"
          >
            {slide.title}
          </motion.h1>
          <motion.p
            key={`${index}-subtitle`}
            initial={{ opacity: 0, y: 18 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.08, duration: 0.45 }}
            className="mt-6 max-w-2xl text-base leading-8 text-white/80 md:text-lg"
          >
            {slide.subtitle}
          </motion.p>

          <div className="mt-9 flex flex-col gap-3 sm:flex-row">
            <a
              href="#core-tech"
              className="inline-flex min-h-11 items-center justify-center rounded-full bg-[var(--default-color)] px-6 text-sm font-semibold text-white transition hover:bg-[var(--default-color-hover)] hover:text-white"
            >
              查看核心技术
            </a>
            <a
              href="#cases"
              className="inline-flex min-h-11 items-center justify-center rounded-full border border-white/30 bg-white/10 px-6 text-sm font-semibold text-white transition hover:border-white/50 hover:bg-white/20 hover:text-white"
            >
              浏览示范案例
            </a>
          </div>
        </div>

        <ChainIntelligenceVisual activeIndex={index} />
      </div>

      <button
        aria-label="上一张"
        onClick={() => paginate(-1)}
        className="absolute left-4 top-1/2 z-10 hidden h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full border border-white/20 bg-black/25 text-white backdrop-blur transition hover:bg-white/20 md:flex"
      >
        <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5 8.25 12l7.5-7.5" />
        </svg>
      </button>
      <button
        aria-label="下一张"
        onClick={() => paginate(1)}
        className="absolute right-4 top-1/2 z-10 hidden h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full border border-white/20 bg-black/25 text-white backdrop-blur transition hover:bg-white/20 md:flex"
      >
        <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" d="m8.25 4.5 7.5 7.5-7.5 7.5" />
        </svg>
      </button>

      <div className="absolute bottom-6 left-1/2 z-10 flex -translate-x-1/2 items-center gap-2">
        {slides.map((item, i) => (
          <button
            key={item.title}
            aria-label={`切换到第 ${i + 1} 张`}
            aria-current={i === index}
            onClick={() => goTo(i)}
            className={`h-2.5 rounded-full transition-all duration-300 ${
              i === index ? 'w-7 bg-white' : 'w-2.5 bg-white/40 hover:bg-white/70'
            }`}
          />
        ))}
      </div>
    </section>
  )
}

function ChainIntelligenceVisual({ activeIndex }: { activeIndex: number }) {
  const rows = ['链上交易图谱', '地址聚类分析', '异常资金路径']

  return (
    <motion.div
      initial={{ opacity: 0, y: 28 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.12, duration: 0.55 }}
      className="hidden lg:block"
    >
      <div className="relative min-h-[420px]">
        <div className="absolute inset-8 rounded-lg border border-white/20 bg-white/10 backdrop-blur-md" />
        <div className="absolute left-4 top-0 w-[78%] rounded-lg border border-white/20 bg-[#101820]/75 p-5 text-white shadow-2xl backdrop-blur-xl">
          <div className="flex items-center justify-between border-b border-white/12 pb-4">
            <div>
              <div className="text-xs text-white/60">CHAIN INTELLIGENCE</div>
              <div className="mt-1 font-inter text-lg font-semibold">风险监测驾驶舱</div>
            </div>
            <span className="rounded-full bg-[var(--default-color)] px-3 py-1 text-xs font-semibold">
              ONLINE
            </span>
          </div>

          <div className="mt-5 grid grid-cols-[1fr_120px] gap-5">
            <div className="space-y-3">
              {rows.map((row, i) => (
                <div
                  key={row}
                  className={`rounded-lg border p-3 transition ${
                    i === activeIndex
                      ? 'border-[var(--default-color)] bg-[var(--default-color)]/20'
                      : 'border-white/12 bg-white/6'
                  }`}
                >
                  <div className="flex items-center justify-between text-sm">
                    <span>{row}</span>
                    <span className="text-white/50">0{i + 1}</span>
                  </div>
                  <div className="mt-3 h-1.5 rounded-full bg-white/12">
                    <div
                      className="h-full rounded-full bg-[var(--accent-color)] transition-all duration-500"
                      style={{ width: `${56 + i * 14}%` }}
                    />
                  </div>
                </div>
              ))}
            </div>

            <div className="relative rounded-lg border border-white/12 bg-white/6">
              {[0, 1, 2, 3, 4].map((node) => (
                <span
                  key={node}
                  className="absolute h-3 w-3 rounded-full bg-[var(--default-color)] shadow-[0_0_22px_var(--default-color)]"
                  style={{
                    left: `${18 + ((node * 31) % 62)}%`,
                    top: `${18 + ((node * 23) % 58)}%`,
                  }}
                />
              ))}
              <div className="absolute inset-x-4 top-1/2 h-px bg-gradient-to-r from-transparent via-white/40 to-transparent" />
              <div className="absolute inset-y-4 left-1/2 w-px bg-gradient-to-b from-transparent via-white/30 to-transparent" />
            </div>
          </div>
        </div>

        <div className="absolute bottom-8 right-0 w-[58%] rounded-lg border border-white/20 bg-[#f7efe7]/95 p-4 text-[#17262b] shadow-2xl">
          <div className="text-xs font-semibold text-[#86572c]">联合研判流转</div>
          <div className="mt-3 flex items-center gap-2">
            {['监测', '识别', '预警', '处置'].map((step, i) => (
              <div key={step} className="flex flex-1 items-center gap-2">
                <div className="flex h-8 w-8 items-center justify-center rounded-full bg-[#bd7c40] text-xs font-bold text-white">
                  {i + 1}
                </div>
                <span className="text-xs font-semibold">{step}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </motion.div>
  )
}

