import { createRouter, createWebHashHistory } from 'vue-router'
import AppLayout from '@/layouts/AppLayout.vue'

/**
 * 顶层仅 3 个路由：/chat、/settings、/skills（计划 §三）
 * 使用 hash history：Tauri 生产环境以 file:// 加载前端资源，hash 模式无需服务端回退
 */
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      component: AppLayout,
      redirect: '/chat',
      children: [
        {
          path: 'chat',
          name: 'chat',
          component: () => import('@/views/ChatView/index.vue'),
        },
        {
          path: 'settings',
          name: 'settings',
          component: () => import('@/views/SettingsView/index.vue'),
        },
        {
          path: 'skills',
          name: 'skills',
          component: () => import('@/views/SkillsView/index.vue'),
        },
      ],
    },
  ],
})

export default router
