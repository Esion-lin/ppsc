import type { Metadata } from 'next'
import Navbar from '@/components/Navbar'
import PpscDemo from '@/components/PpscDemo'

export const metadata: Metadata = {
  title: '课题一 · PPSC 隐私保护智能合约演示',
  description: '在本地链部署编译合约，通过常驻 daemon 与 OpenFHE BFV / Shamir 执行实际加密入账、转账和出账。',
}

export default function TopicOnePage() {
  return <><Navbar /><PpscDemo /></>
}
