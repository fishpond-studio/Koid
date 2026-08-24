import { defineConfig } from 'vitepress'

// Koid 文档站点（§4 Phase 4）
export default defineConfig({
  title: 'Koid',
  description: 'Not just code. It\'s Koid. — Open-source Vibe Coding desktop agent',
  lang: 'zh-CN',
  themeConfig: {
    logo: '/logo.png',
    nav: [
      { text: '下载', link: '/download' },
      { text: '指南', link: '/guide/intro' },
      { text: '模块', link: '/modules/skills' },
      { text: '参考', link: '/reference/plugin-api' },
    ],
    sidebar: [
      {
        text: '指南',
        items: [
          { text: '介绍', link: '/guide/intro' },
          { text: '快速开始', link: '/guide/quickstart' },
          { text: 'macOS 安装', link: '/guide/macos-install' },
          { text: '架构', link: '/guide/architecture' },
          { text: '数据与安全', link: '/guide/data-security' },
        ],
      },
      {
        text: '功能模块',
        items: [
          { text: '工作区', link: '/modules/workspace' },
          { text: '主题系统', link: '/modules/theme' },
          { text: '多供应商模型', link: '/modules/models' },
          { text: '故障转移', link: '/modules/failover' },
          { text: '代理系统', link: '/modules/proxy' },
          { text: '会话管理', link: '/modules/sessions' },
          { text: '对话页', link: '/modules/chat' },
          { text: '提示词库', link: '/modules/prompts' },
          { text: 'Skills', link: '/modules/skills' },
          { text: 'MCP', link: '/modules/mcp' },
          { text: '插件', link: '/modules/plugins' },
          { text: '关闭行为与托盘', link: '/modules/tray' },
        ],
      },
      {
        text: '参考',
        items: [
          { text: '插件 API', link: '/reference/plugin-api' },
          { text: 'Skill YAML', link: '/reference/skill-yaml' },
          { text: '错误码', link: '/reference/errors' },
        ],
      },
    ],
    footer: {
      message: 'Released by Fishpond Studio. Not just code. It\'s Koid.',
    },
  },
})
