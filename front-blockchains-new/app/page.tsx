import Navbar from '@/components/Navbar'
import HeroCarousel from '@/components/HeroCarousel'
import CoreTech from '@/components/CoreTech'
import Cases from '@/components/Cases'

export default function Home() {
  return (
    <div className="min-h-screen">
      <Navbar />

      <main>
        {/* 一、Banner 轮播 */}
        <HeroCarousel />

        {/* 二、核心技术 */}
        <CoreTech />

        {/* 三、应用案例 */}
        <Cases />
      </main>

      <footer className="border-t border-[var(--default-border-color)] bg-[var(--secondary-background)] py-4">
        <div className="mx-auto flex max-w-7xl flex-col gap-2 px-5 text-xs text-[var(--desc-color)] md:flex-row md:items-center md:justify-between md:px-6">
          <p>© {new Date().getFullYear()} 版权所有</p>
          <p>区块链XX及XX示范平台</p>
        </div>
      </footer>
    </div>
  )
}
