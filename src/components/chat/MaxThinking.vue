<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'

/**
 * 思考动画（仅思考强度 = 最大时启用）：
 * Claude 风格璀璨星（渐变四角星光晕呼吸 + 环绕微光闪烁）
 * + 流光词条轮换 + 计秒。词条由 i18n 竖线分隔配置。
 */
defineProps<{ seconds: number }>()

const { t } = useI18n()

/** 渐变 id 加随机后缀，防多实例/多页面 id 冲突 */
const gid = `mtg-${Math.random().toString(36).slice(2, 8)}`

const phrase = ref(0)

function phrases(): string[] {
  return t('chat.maxThinkingWords').split('|').filter(Boolean)
}

const phraseTimer = window.setInterval(() => {
  const list = phrases()
  if (list.length > 0) phrase.value = (phrase.value + 1) % list.length
}, 2200)

onBeforeUnmount(() => window.clearInterval(phraseTimer))
</script>

<template>
  <span class="inline-flex items-center gap-2">
    <!-- 璀璨星：主星呼吸旋转 + 渐变流转，双微光交错闪烁 -->
    <span class="star-wrap relative inline-flex size-4 shrink-0 items-center justify-center">
      <svg viewBox="0 0 24 24" class="star-main size-4" aria-hidden="true">
        <defs>
          <linearGradient :id="gid" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#F5B99B" />
            <stop offset="50%" stop-color="#D97757" />
            <stop offset="100%" stop-color="#A84B31" />
            <animateTransform
              attributeName="gradientTransform"
              type="rotate"
              from="0 0.5 0.5"
              to="360 0.5 0.5"
              dur="2.6s"
              repeatCount="indefinite"
            />
          </linearGradient>
        </defs>
        <path
          :fill="`url(#${gid})`"
          d="M12 1.6C12.9 7.6 16.4 11.1 22.4 12c-6 .9-9.5 4.4-10.4 10.4C11.1 16.4 7.6 12.9 1.6 12c6-.9 9.5-4.4 10.4-10.4Z"
        />
      </svg>
      <span class="sparkle sparkle-a" />
      <span class="sparkle sparkle-b" />
    </span>

    <Transition name="mt-word" mode="out-in">
      <span :key="phrase" class="shimmer-text whitespace-nowrap">
        {{ phrases()[phrase] ?? '' }}
      </span>
    </Transition>

    <span
      v-if="seconds > 0"
      class="shrink-0 tabular-nums text-muted-foreground/70"
    >
      {{ seconds }}s
    </span>
  </span>
</template>

<style scoped>
.star-wrap {
  transform-origin: center;
}

.star-main {
  animation: mt-star-pulse 1.9s ease-in-out infinite;
  filter: drop-shadow(0 0 6px rgb(217 119 87 / 45%));
}

/* 主星：缩放呼吸 + 轻微摇摆（非闪烁） */
@keyframes mt-star-pulse {
  0%,
  100% {
    transform: scale(0.82) rotate(-6deg);
    filter: drop-shadow(0 0 4px rgb(217 119 87 / 35%));
  }
  50% {
    transform: scale(1.08) rotate(8deg);
    filter: drop-shadow(0 0 9px rgb(217 119 87 / 60%));
  }
}

.sparkle {
  position: absolute;
  border-radius: 9999px;
  background: #e8b48f;
}
.sparkle-a {
  right: -1px;
  top: -1px;
  width: 4px;
  height: 4px;
  animation: mt-twinkle 1.9s ease-in-out infinite;
}
.sparkle-b {
  left: -2px;
  bottom: 0;
  width: 3px;
  height: 3px;
  animation: mt-twinkle 1.9s ease-in-out infinite;
  animation-delay: 0.95s;
}

@keyframes mt-twinkle {
  0%,
  100% {
    opacity: 0;
    transform: scale(0.4);
  }
  50% {
    opacity: 1;
    transform: scale(1);
  }
}

.shimmer-text {
  background: linear-gradient(
    100deg,
    hsl(var(--muted-foreground)) 25%,
    hsl(var(--primary)) 50%,
    hsl(var(--muted-foreground)) 75%
  );
  background-size: 250% 100%;
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
  /* 匀速流光（非闪烁），配合透明度过渡的词条轮换 */
  animation: mt-shimmer 2.2s linear infinite;
}

@keyframes mt-shimmer {
  from {
    background-position: 180% 0;
  }
  to {
    background-position: -80% 0;
  }
}

.mt-word-enter-active,
.mt-word-leave-active {
  transition:
    opacity 0.18s ease,
    transform 0.18s ease;
}
.mt-word-enter-from {
  opacity: 0;
  transform: translateY(4px);
}
.mt-word-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

@media (prefers-reduced-motion: reduce) {
  .star-main,
  .sparkle,
  .shimmer-text {
    animation: none;
  }
  .shimmer-text {
    background: none;
    color: hsl(var(--foreground));
  }
  .mt-word-enter-active,
  .mt-word-leave-active {
    transition: none;
  }
}
</style>
