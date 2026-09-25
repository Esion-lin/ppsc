// =========================================================================
// 站点内容配置（占位文案集中在此文件，后续替换为真实内容即可）
// =========================================================================

export const siteTitle = '区块链XX及XX示范平台'
export const siteSubtitle =
  '面向全球公有链开展实时监测、地址分析与资金追踪的区块链示范平台'

// -------------------------------------------------------------------------
// 顶部导航栏
// 有 children 的项渲染为下拉菜单；无 href 的子项仅作展示（不跳转）
// -------------------------------------------------------------------------
export type NavItem = {
  label: string
  href?: string
  children?: NavItem[]
}

export const navItems: NavItem[] = [
  { label: '首页', href: '/#home' },
  { label: '课题一 · PPSC', href: '/topic-one' },
  {
    label: '公有链风险监测',
    children: [
      { label: '全球公有链实时监测' },
      { label: '公有链资金追踪' },
      { label: '链上风险智能体' },
      { label: '智能合约分析' },
      { label: '课题X页面' },
    ],
  },
  {
    label: '联盟链运维（预留）',
    children: [
      { label: '课题X页面' },
      { label: '课题X页面' },
    ],
  },
  {
    label: '示范应用',
    children: [
      { label: '小众担保监控' },
      {
        label: '智能追踪案例集',
        children: [
          { label: '案例1' },
          { label: '案例2' },
        ],
      },
    ],
  },
]

// -------------------------------------------------------------------------
// 一、顶部 Banner 轮播（3 张）
// -------------------------------------------------------------------------
export type Slide = {
  title: string
  subtitle: string
  /** 高清背景图片 URL，留空时使用品牌渐变背景 */
  image?: string
}

export const slides: Slide[] = [
  {
    title: '洞察链上脉络，构建智能追踪新能力',
    subtitle:
      '面向全球公有链开展实时监测、地址分析与资金追踪，提升区块链风险发现和研判能力。',
  },
  {
    title: '汇聚关键技术，驱动区块链发展',
    subtitle:
      '聚焦技术创新与融合应用，持续提升平台能力，为区块链高质量发展提供有力支撑。',
  },
  {
    title: '深化场景应用，打造区块链示范标杆',
    subtitle:
      '围绕重点业务需求建设典型应用场景，推动技术成果验证、业务落地与规模化推广。',
  },
]

// -------------------------------------------------------------------------
// 二、核心技术板块（4 个，左右分栏交替）
// layout: 'left' 表示左图右文，'right' 表示左文右图
// -------------------------------------------------------------------------
export type TechTopic = {
  href?: string
  title: string
  description: string
  layout: 'left' | 'right'
  /** 示意图 URL，留空时使用品牌渐变占位块 */
  image?: string
}

export const techTopics: TechTopic[] = [
  {
    title: '课题一：隐私保护智能合约 PPSC',
    description:
      '基于 OpenFHE BFV 与 Shamir 分享，在本地链部署 .ppsc 编译合约，展示加密输入上传、常驻 Runtime 自动计算与链上结果回写。通过入账、转账和出账验证账户状态；当前密码委员会在同一进程内运行。',
    href: '/topic-one',
    layout: 'left',
  },
  {
    title: 'XX关键技术二研究',
    description:
      '请填写约150字的课题介绍，说明课题的研究背景、建设目标、关键技术、主要任务以及预期成果，突出该课题在整个项目中的定位和作用。',
    layout: 'right',
  },
  {
    title: 'XX平台一研发',
    description:
      '请填写约150字的课题介绍，重点说明平台的总体架构、核心功能、技术特点、服务对象以及平台能够解决的主要业务问题。',
    layout: 'left',
  },
  {
    title: 'XX平台二研发',
    description:
      '请填写约150字的课题介绍，重点说明平台的总体架构、核心功能、技术特点、服务对象以及平台能够解决的主要业务问题。',
    layout: 'right',
  },
]

// -------------------------------------------------------------------------
// 三、应用案例（3 个，左右翻页轮播）
// -------------------------------------------------------------------------
export type CaseItem = {
  name: string
  description: string
  tags?: string[]
  /** 是否展示「查看案例详情」按钮 */
  showDetail?: boolean
  /** 案例配图 URL，留空时使用品牌渐变占位块 */
  image?: string
}

export const caseItems: CaseItem[] = [
  {
    name: 'XXXXXXX应用案例一',
    description:
      '请填写约200字的案例介绍，说明案例的应用背景、用户需求、解决方案、使用的关键技术以及实际应用效果。',
    tags: ['示范应用', '行业实践'],
    showDetail: true,
  },
  {
    name: 'XXXXXXX应用案例二',
    description:
      '请填写约200字的案例介绍，说明案例的应用背景、用户需求、解决方案、使用的关键技术以及实际应用效果。',
    tags: ['典型案例'],
    showDetail: true,
  },
  {
    name: 'XXXXXXX应用案例三',
    description:
      '请填写约200字的案例介绍，说明案例的应用背景、用户需求、解决方案、使用的关键技术以及实际应用效果。',
    tags: ['示范应用'],
    showDetail: true,
  },
]
