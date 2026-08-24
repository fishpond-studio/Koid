<script setup lang="ts">
import { RouterView } from 'vue-router'
</script>

<template>
  <!-- 无侧边栏布局：供全屏场景（如插件安装向导）预留 -->
  <div class="h-screen overflow-hidden">
    <RouterView />
  </div>
</template>
