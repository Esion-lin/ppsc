import type { Metadata } from 'next'
import WalletDashboard from '@/components/WalletDashboard'
import './wallet.css'

export const metadata: Metadata = { title: 'Wallet · PPSC', description: '在本地 Anvil 链验证 PPSC 账户、入账与转账，上传编译隐私合约；支持 OpenFHE 真实密码后端，需按文档启动 daemon。' }
export default function WalletPage() { return <WalletDashboard /> }
