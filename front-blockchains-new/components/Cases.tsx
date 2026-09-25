'use client'

import { AnimatePresence, motion, type PanInfo } from 'framer-motion'
import { useState } from 'react'
import { caseItems } from '@/lib/content'
import SectionHeading from './SectionHeading'

const variants = {
  enter: (dir: number) => ({ x: dir > 0 ? 44 : -44, opacity: 0 }),
  center: { x: 0, opacity: 1 },
  exit: (dir: number) => ({ x: dir > 0 ? -44 : 44, opacity: 0 }),
}

const caseMeta = [
  { area: '跨链追踪', status: '联调中', score: 86 },
  { area: '风险预警', status: '试运行', score: 79 },
  { area: '协同处置', status: '建设中', score: 72 },
]

export default function Cases() {
  const [[index, direction], setIndex] = useState<[number, number]>([0, 0])

  const paginate = (dir: number) =>
    setIndex(([i]) => [(i + dir + caseItems.length) % caseItems.length, dir])

  const goTo = (i: number) => setIndex(([cur]) => [i, i === cur ? 0 : i > cur ? 1 : -1])

  const onDragEnd = (_event: MouseEvent | TouchEvent | PointerEvent, info: PanInfo) => {
    if (info.offset.x > 70) paginate(-1)
    if (info.offset.x < -70) paginate(1)
  }

  const current = caseItems[index]
  const meta = caseMeta[index % caseMeta.length]

  return (
    <section id="cases" className="mx-auto max-w-7xl px-5 py-24 md:px-6">
      <SectionHeading
        title="应用案例"
        subtitle="典型应用场景与示范成果，推动技术验证、业务落地与规模化推广。"
      />

      <div className="grid gap-6 lg:grid-cols-[280px_1fr]">
        <aside className="surface h-fit rounded-lg p-3">
          <div className="px-3 pb-3 pt-2">
            <div className="text-xs font-semibold text-[var(--desc-color)]">案例目录</div>
            <div className="mt-1 font-inter text-lg font-bold">示范应用矩阵</div>
          </div>
          <div className="space-y-2">
            {caseItems.map((item, i) => (
              <button
                key={item.name}
                aria-current={i === index}
                onClick={() => goTo(i)}
                className={`flex min-h-16 w-full items-center gap-3 rounded-lg border px-3 text-left transition ${
                  i === index
                    ? 'border-[var(--default-color)] bg-[var(--list-item-hover-color)] text-[var(--default-color)]'
                    : 'border-transparent hover:border-[var(--default-border-color)] hover:bg-[var(--glass-background)]'
                }`}
              >
                <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-[var(--secondary-background)] text-sm font-bold">
                  {String(i + 1).padStart(2, '0')}
                </span>
                <span>
                  <span className="block text-sm font-semibold">{item.name}</span>
                  <span className="mt-0.5 block text-xs text-[var(--desc-color)]">
                    {caseMeta[i % caseMeta.length].area}
                  </span>
                </span>
              </button>
            ))}
          </div>
        </aside>

        <div className="relative min-h-[520px] overflow-hidden rounded-lg">
          <AnimatePresence initial={false} custom={direction} mode="wait">
            <motion.article
              key={index}
              custom={direction}
              variants={variants}
              initial="enter"
              animate="center"
              exit="exit"
              drag="x"
              dragConstraints={{ left: 0, right: 0 }}
              dragElastic={0.08}
              onDragEnd={onDragEnd}
              transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
              className="surface grid min-h-[520px] cursor-grab overflow-hidden rounded-lg active:cursor-grabbing md:grid-cols-[0.92fr_1.08fr]"
            >
              <CaseVisual image={current.image} index={index} score={meta.score} />

              <div className="flex flex-col justify-center border-t border-[var(--default-border-color)] p-6 md:border-l md:border-t-0 md:p-10">
                <div className="flex flex-wrap gap-2">
                  {current.tags?.map((tag) => (
                    <span
                      key={tag}
                      className="rounded-full border border-[var(--default-border-color)] bg-[var(--glass-background)] px-3 py-1 text-xs font-semibold text-[var(--default-color)]"
                    >
                      {tag}
                    </span>
                  ))}
                  <span className="rounded-full bg-[var(--default-color)] px-3 py-1 text-xs font-semibold text-white">
                    {meta.status}
                  </span>
                </div>

                <h3 className="mt-5 font-inter text-2xl font-bold leading-tight md:text-4xl">
                  {current.name}
                </h3>
                <p className="mt-5 text-base leading-8 text-[var(--desc-color)]">
                  {current.description}
                </p>

                <div className="mt-8 grid gap-3 sm:grid-cols-3">
                  {['需求识别', '方案验证', '成果推广'].map((step, i) => (
                    <div
                      key={step}
                      className="rounded-lg border border-[var(--default-border-color)] bg-[var(--secondary-background)] p-4"
                    >
                      <div className="flex h-8 w-8 items-center justify-center rounded-full bg-[var(--other-background)] text-xs font-bold text-[var(--default-color)]">
                        {i + 1}
                      </div>
                      <div className="mt-3 text-sm font-semibold">{step}</div>
                    </div>
                  ))}
                </div>

                <div className="mt-8 flex flex-col gap-3 sm:flex-row">
                  {current.showDetail && (
                    <button className="btn-primary">查看案例详情</button>
                  )}
                  <button
                    onClick={() => paginate(1)}
                    className="inline-flex min-h-11 items-center justify-center rounded-full border border-[var(--default-border-color)] px-6 text-sm font-semibold text-[var(--default-color)] transition hover:border-[var(--default-color)] hover:bg-[var(--list-item-hover-color)]"
                  >
                    下一个案例
                  </button>
                </div>
              </div>
            </motion.article>
          </AnimatePresence>

          <button
            aria-label="上一个案例"
            onClick={() => paginate(-1)}
            className="absolute left-4 top-1/2 z-10 hidden h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full border border-[var(--default-border-color)] bg-[var(--secondary-background)] text-[var(--default-text-color)] shadow-lift transition hover:border-[var(--default-color)] hover:text-[var(--default-color)] md:flex"
          >
            <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5 8.25 12l7.5-7.5" />
            </svg>
          </button>
          <button
            aria-label="下一个案例"
            onClick={() => paginate(1)}
            className="absolute right-4 top-1/2 z-10 hidden h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full border border-[var(--default-border-color)] bg-[var(--secondary-background)] text-[var(--default-text-color)] shadow-lift transition hover:border-[var(--default-color)] hover:text-[var(--default-color)] md:flex"
          >
            <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="m8.25 4.5 7.5 7.5-7.5 7.5" />
            </svg>
          </button>
        </div>
      </div>
    </section>
  )
}

function CaseVisual({
  image,
  index,
  score,
}: {
  image?: string
  index: number
  score: number
}) {
  if (image) {
    return (
      <div
        className="min-h-[320px] bg-cover bg-center md:min-h-full"
        style={{ backgroundImage: `url(${image})` }}
      />
    )
  }

  return (
    <div className="relative min-h-[320px] overflow-hidden bg-[linear-gradient(135deg,var(--hero-from),var(--hero-via),var(--hero-to))] p-6 text-white md:min-h-full">
      <div className="relative flex h-full min-h-[320px] flex-col justify-between">
        <div className="flex items-center justify-between">
          <span className="rounded-full bg-white/20 px-3 py-1 text-xs font-semibold">
            CASE-{String(index + 1).padStart(2, '0')}
          </span>
          <span className="text-xs text-white/70">可拖拽切换</span>
        </div>

        <div className="mx-auto grid w-full max-w-[340px] grid-cols-3 gap-3">
          {[0, 1, 2].map((item) => (
            <div key={item} className="rounded-lg border border-white/20 bg-white/10 p-3 backdrop-blur">
              <div className="h-16 rounded-md bg-white/20" />
              <div className="mt-3 h-1.5 rounded-full bg-white/20">
                <div
                  className="h-full rounded-full bg-[var(--default-color)]"
                  style={{ width: `${score - item * 12}%` }}
                />
              </div>
            </div>
          ))}
        </div>

        <div>
          <div className="flex justify-between text-sm">
            <span>应用成熟度</span>
            <span className="font-semibold">{score}%</span>
          </div>
          <div className="mt-3 h-2 rounded-full bg-white/20">
            <div
              className="h-full rounded-full bg-white transition-all duration-500"
              style={{ width: `${score}%` }}
            />
          </div>
        </div>
      </div>
    </div>
  )
}
