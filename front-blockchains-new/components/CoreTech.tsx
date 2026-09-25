'use client'

import { motion } from 'framer-motion'
import Link from 'next/link'
import { techTopics } from '@/lib/content'
import SectionHeading from './SectionHeading'

const techMeta = [
  { label: '隐私计算', level: 82 },
  { label: '智能研判', level: 76 },
  { label: '平台支撑', level: 88 },
  { label: '应用验证', level: 72 },
]

export default function CoreTech() {
  return (
    <section
      id="core-tech"
      className="relative overflow-hidden border-y border-[var(--default-border-color)] bg-[var(--other-background)] py-24"
    >
      <div className="mx-auto max-w-7xl px-5 md:px-6">
        <SectionHeading
          title="核心技术"
          subtitle="围绕关键技术与平台研发，构建区块链风险监测与示范应用的核心能力。"
        />

        <div className="space-y-6">
          {techTopics.map((topic, i) => {
            const isLeft = topic.layout === 'left'
            const meta = techMeta[i % techMeta.length]

            return (
              <motion.article
                key={topic.title}
                initial={{ opacity: 0, y: 34 }}
                whileInView={{ opacity: 1, y: 0 }}
                whileHover={{ y: -3 }}
                viewport={{ once: true, margin: '-80px' }}
                transition={{ duration: 0.45 }}
                className="grid overflow-hidden rounded-lg bg-[var(--panel-background)] shadow-lift backdrop-blur-xl md:grid-cols-2"
              >
                <div className={`${isLeft ? 'md:order-1' : 'md:order-2'} p-5 md:p-8`}>
                  <TechVisual index={i} meta={meta} image={topic.image} />
                </div>

                <div
                  className={`${isLeft ? 'md:order-2' : 'md:order-1'} flex flex-col justify-center p-6 md:p-10`}
                >
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="flex h-10 w-10 items-center justify-center rounded-lg bg-[var(--default-color)] text-sm font-bold text-white">
                      {String(i + 1).padStart(2, '0')}
                    </span>
                    <span className="rounded-full bg-[var(--glass-background)] px-3 py-1 text-xs font-semibold text-[var(--default-color)]">
                      {meta.label}
                    </span>
                  </div>

                  <h3 className="mt-6 font-inter text-2xl font-bold leading-tight md:text-3xl">
                    {topic.title}
                  </h3>
                  <p className="mt-5 text-base leading-8 text-[var(--desc-color)]">
                    {topic.description}
                  </p>
                  {topic.href && <Link href={topic.href} className="mt-6 inline-flex min-h-11 items-center font-semibold text-[var(--default-color)]">进入课题演示 <span className="ml-2" aria-hidden="true">→</span></Link>}
                </div>
              </motion.article>
            )
          })}
        </div>
      </div>
    </section>
  )
}

function TechVisual({
  index,
  meta,
  image,
}: {
  index: number
  meta: { label: string; level: number }
  image?: string
}) {
  if (image) {
    return (
      <div
        className="aspect-[4/3] rounded-lg bg-cover bg-center shadow-lift"
        style={{ backgroundImage: `url(${image})` }}
      />
    )
  }

  return (
    <div className="relative aspect-[4/3] overflow-hidden rounded-lg bg-[linear-gradient(135deg,var(--hero-from),var(--hero-via),var(--hero-to))] p-5 text-white shadow-lift">
      <div className="relative flex h-full flex-col justify-between">
        <div className="flex items-start justify-between">
          <div>
            <div className="text-xs text-white/70">TECH MODULE</div>
            <div className="mt-2 font-inter text-2xl font-bold">{meta.label}</div>
          </div>
          <span className="rounded-full bg-white/20 px-3 py-1 text-xs font-semibold">
            M-{String(index + 1).padStart(2, '0')}
          </span>
        </div>

        <div className="mx-auto grid w-[72%] grid-cols-3 items-end gap-3">
          {[46, meta.level, 62].map((value, i) => (
            <div key={i} className="flex flex-col items-center gap-3">
              <div
                className="w-full rounded-t-lg bg-white/80 transition-all duration-300"
                style={{ height: `${value * 1.9}px` }}
              />
              <span className="h-2 w-2 rounded-full bg-[var(--default-color)]" />
            </div>
          ))}
        </div>

        <div>
          <div className="flex justify-between text-xs text-white/70">
            <span>能力成熟度</span>
            <span>{meta.level}%</span>
          </div>
          <div className="mt-2 h-2 rounded-full bg-white/20">
            <div
              className="h-full rounded-full bg-[var(--default-color)] transition-all duration-500"
              style={{ width: `${meta.level}%` }}
            />
          </div>
        </div>
      </div>
    </div>
  )
}
