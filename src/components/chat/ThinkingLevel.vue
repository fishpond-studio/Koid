<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
} from 'reka-ui'
import { Brain } from 'lucide-vue-next'
import type { ThinkingLevel } from '@/types'
import { themeFirePalette, useWebglFire } from './thinking/useWebglFire'

/**
 * 思考强度选择器（复刻 Claude Code Effort 滑块）：
 * Brain 芯片弹出深色卡片，五档离散滑轨；拉到「最大」触发
 * WebGL 能量流（配色运行时读取主题 --primary，切主题自动跟随）。
 */
const props = defineProps<{ level: ThinkingLevel }>()
const emit = defineEmits<{ change: [level: ThinkingLevel] }>()
const { t } = useI18n()

const LEVELS: ThinkingLevel[] = ['default', 'low', 'medium', 'high', 'max']

/* ── 滑块状态 ─────────────────────────── */
const sliderIndex = ref(Math.max(0, LEVELS.indexOf(props.level)))

watch(
  () => props.level,
  (l) => (sliderIndex.value = Math.max(0, LEVELS.indexOf(l))),
)

function onInput(e: Event) {
  const i = parseInt((e.target as HTMLInputElement).value, 10)
  sliderIndex.value = i
  const next = LEVELS[i]
  if (next && next !== props.level) emit('change', next)
}

/** 火焰位置（0..1）：仅最大档 ≥0.95 阈值点燃 */
const firePos = computed(() => (sliderIndex.value / (LEVELS.length - 1)) * 100)
const isActive = computed(() => props.level === 'max')
const isFull = computed(() => sliderIndex.value === LEVELS.length - 1)

/* ── 进入/离开「最大」的翻转动画 ────────── */
const isAnimating = ref(false)
let animTimer: ReturnType<typeof setTimeout> | null = null

watch(isActive, (now, was) => {
  if (now && !was) {
    if (animTimer) clearTimeout(animTimer)
    isAnimating.value = true
    animTimer = setTimeout(() => (isAnimating.value = false), 460)
  }
})

onBeforeUnmount(() => {
  if (animTimer) clearTimeout(animTimer)
})

/* ── squircle 裁剪 id ─────────────────── */
const uid = Math.random().toString(36).slice(2, 8)
const clipId = `squircle-${uid}`
const clipTrackId = `squircle-track-${uid}`
const cardClip = computed(() => ({ clipPath: `url(#${clipId})` }))
const trackClip = computed(() => ({ clipPath: `url(#${clipTrackId})` }))

/** 火焰画布遮罩：只显示滑块左侧区域 */
const canvasMask = computed(() => {
  const p = Math.min(firePos.value + 2, 100)
  return {
    maskImage: `linear-gradient(to right, black 0%, black ${p}%, transparent ${p}%)`,
    WebkitMaskImage: `linear-gradient(to right, black 0%, black ${p}%, transparent ${p}%)`,
  }
})

/* ── WebGL 引擎 ───────────────────────── */
const canvasRef = ref<HTMLCanvasElement | null>(null)
const { setPalette, supported } = useWebglFire(canvasRef, firePos, isActive)

/* ── 主题跟随：html 属性变化时同步暗色状态 + 重读 --primary ── */
const isDark = ref(false)
function refreshPalette() {
  setPalette(themeFirePalette())
}
function syncTheme() {
  isDark.value = document.documentElement.classList.contains('dark')
  refreshPalette()
}
let mo: MutationObserver | null = null
onMounted(() => {
  syncTheme()
  mo = new MutationObserver(syncTheme)
  mo.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class', 'style', 'data-theme'],
  })
})
onBeforeUnmount(() => mo?.disconnect())

/** 刻度点位置（对齐五档停靠位） */
function dotLeft(i: number): string {
  if (i === 0) return '6px'
  if (i === LEVELS.length - 1) return 'calc(100% - 6px)'
  return `${i * 25}%`
}
</script>

<template>
  <!-- squircle 裁剪路径定义 -->
  <svg class="squircle-clip" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
    <defs>
      <clipPath :id="clipId" clipPathUnits="objectBoundingBox">
        <path
          d="M 0.053,0 C 0.029,0 0.012,0.008 0.005,0.02 C 0.002,0.028 0,0.038 0,0.053 L 0,0.947 C 0,0.962 0.002,0.972 0.005,0.98 C 0.012,0.992 0.029,1 0.053,1 L 0.947,1 C 0.971,1 0.988,0.992 0.995,0.98 C 0.998,0.972 1,0.962 1,0.947 L 1,0.053 C 1,0.038 0.998,0.028 0.995,0.02 C 0.988,0.008 0.971,0 0.947,0 Z"
        />
      </clipPath>
      <clipPath :id="clipTrackId" clipPathUnits="objectBoundingBox">
        <path
          d="M 0.033,0 C 0.018,0 0.007,0.012 0.003,0.035 C 0.001,0.055 0,0.1 0,0.15 L 0,0.85 C 0,0.9 0.001,0.945 0.003,0.965 C 0.007,0.988 0.018,1 0.033,1 L 0.967,1 C 0.982,1 0.993,0.988 0.997,0.965 C 0.999,0.945 1,0.9 1,0.85 L 1,0.15 C 1,0.1 0.999,0.055 0.997,0.035 C 0.993,0.012 0.982,0 0.967,0 Z"
        />
      </clipPath>
    </defs>
  </svg>

  <PopoverRoot>
    <PopoverTrigger
      class="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
      :title="t('chat.thinkingLevel')"
    >
      <Brain class="size-3.5 shrink-0" />
      <!-- 固定宽度：档位文字长度变化时芯片与弹层锚点不漂移 -->
      <span class="inline-block w-12 text-center">
        {{ t(`chat.thinkingLevels.${level}`) }}
      </span>
    </PopoverTrigger>

    <PopoverPortal>
      <PopoverContent
        align="start"
        :side-offset="8"
        class="z-50 w-[19rem] border-0 bg-transparent p-0 shadow-none outline-none"
      >
        <div class="card-shadow" :class="{ 'is-dark': isDark }">
          <div class="card" :class="{ 'is-dark': isDark }" :style="cardClip">
            <!-- 头部：标题 + 当前档位（进最大档时翻转发光） -->
            <div class="header">
              <div class="header-left">
                <span class="label-text">{{ t('chat.thinkingLevel') }}</span>
                <span
                  class="status-text"
                  :class="{ glowing: isActive, 'animate-up': isAnimating }"
                >
                  {{ t(`chat.thinkingLevels.${level}`) }}
                </span>
              </div>
            </div>

            <!-- 两端刻度文案 -->
            <div class="scale-labels">
              <span>{{ t('chat.effortFaster') }}</span>
              <span>{{ t('chat.effortSmarter') }}</span>
            </div>

            <!-- 滑轨：底纹 + 刻度点 + 火焰画布 + 滑块 -->
            <div
              class="track-wrapper"
              :class="{ active: isActive, full: isFull }"
              :style="trackClip"
            >
              <div class="track-bg"></div>
              <div class="dots-layer">
                <span
                  v-for="(l, i) in LEVELS"
                  :key="l"
                  class="dot"
                  :style="{ left: dotLeft(i) }"
                ></span>
              </div>
              <canvas ref="canvasRef" :style="canvasMask"></canvas>
              <!-- WebGL 不可用时的 CSS 兜底：同色系流光 -->
              <div v-if="!supported" class="fire-fallback" :style="canvasMask"></div>
              <input
                type="range"
                min="0"
                max="4"
                step="1"
                :value="sliderIndex"
                :class="{ glowing: isActive }"
                @input="onInput"
              />
            </div>
          </div>
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<style scoped>
.squircle-clip {
  position: absolute;
  width: 0;
  height: 0;
  pointer-events: none;
}

/* ── 卡片外壳：浅色为默认，.is-dark 显式覆盖（运行时检测，不依赖选择器穿透 Portal）── */
.card-shadow {
  filter:
    drop-shadow(0 10px 24px rgb(0 0 0 / 14%))
    drop-shadow(0 3px 8px rgb(0 0 0 / 8%));
}

.card {
  background: #ffffff;
  border: 1px solid rgb(0 0 0 / 12%);
  border-radius: 18px;
  padding: 14px 16px 12px;
  user-select: none;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 13px;
  font-weight: 500;
  perspective: 280px;
  perspective-origin: center 120%;
}

.label-text {
  color: #27272a;
  font-weight: 700;
  line-height: 1.3;
}

.status-text {
  display: inline-block;
  color: #52525b;
  transition:
    color 0.3s,
    text-shadow 0.3s;
  will-change: transform, opacity, filter;
  vertical-align: middle;
  transform-origin: center bottom;
  transform: rotateX(0deg) translateY(0);
}

.scale-labels {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  font-weight: 800;
  color: #71717a;
  margin-bottom: 6px;
  letter-spacing: 0.04em;
}

.card-shadow.is-dark {
  filter:
    drop-shadow(0 12px 28px rgb(0 0 0 / 20%))
    drop-shadow(0 4px 12px rgb(0 0 0 / 10%));
}

.card.is-dark {
  background: #000000;
  border-color: rgb(255 255 255 / 12%);
}

.card.is-dark .label-text {
  color: #b0b0c7;
}

.card.is-dark .status-text:not(.glowing) {
  color: #a1a1aa;
}

.card.is-dark .scale-labels {
  color: #b0b0b8;
}

/* 发光色跟随主题 --primary */
.status-text.glowing {
  color: hsl(var(--primary));
  text-shadow: 0 0 12px hsl(var(--primary) / 60%);
  font-weight: 600;
}

@keyframes flipUpFromBottom {
  0% {
    opacity: 0;
    transform: translateY(18px) rotateX(-80deg);
    filter: blur(4px);
  }
  100% {
    opacity: 1;
    transform: translateY(0) rotateX(0deg);
    filter: blur(0);
  }
}

.status-text.animate-up {
  animation: flipUpFromBottom 0.42s cubic-bezier(0.33, 1, 0.68, 1) forwards;
}

/* ── 滑轨 ─────────────────────────────── */
.track-wrapper {
  position: relative;
  height: 26px;
  overflow: hidden;
  isolation: isolate;
  background: #0c0c0c;
  border: 1px solid #1a1a1e;
}

.track-bg {
  position: absolute;
  inset: 0;
  background: linear-gradient(135deg, #111113, #0a0a0b);
  z-index: 0;
}

/* canvas 始终保持布局尺寸（opacity 而非 display:none） */
canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  opacity: 0;
  mix-blend-mode: screen;
  z-index: 2;
  transition: opacity 0.3s;
}

.track-wrapper.active canvas {
  opacity: 1;
  z-index: 4;
}

/* WebGL 兜底流光（同遮罩、同混合模式） */
.fire-fallback {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 2;
  mix-blend-mode: screen;
  opacity: 0;
  transition: opacity 0.3s;
  background: linear-gradient(
    90deg,
    transparent,
    hsl(var(--primary) / 55%) 40%,
    hsl(var(--primary)) 55%,
    hsl(var(--primary) / 30%) 75%,
    transparent
  );
  background-size: 220% 100%;
  animation: ff-sweep 2.2s linear infinite;
}

.track-wrapper.active .fire-fallback {
  opacity: 1;
}

@keyframes ff-sweep {
  from {
    background-position: 120% 0;
  }
  to {
    background-position: -60% 0;
  }
}

.dots-layer {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 1;
}

.dot {
  position: absolute;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #494950;
  top: 50%;
  transform: translate(-50%, -50%);
  transition: opacity 0.6s;
}

.track-wrapper.active .dot {
  opacity: 0.25;
}
.track-wrapper.full .dot {
  opacity: 0;
}

/* ── range input ─────────────────────── */
input[type='range'] {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  background: transparent;
  appearance: none;
  -webkit-appearance: none;
  cursor: pointer;
  z-index: 5;
  outline: none;
  margin: 0;
  padding: 0;
}

input[type='range']::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 24px;
  height: 24px;
  border-radius: 8px;
  background: linear-gradient(170deg, #ffffff 0%, #f0f0f2 40%, #e4e4e6 100%);
  border: 0.5px solid rgb(0 0 0 / 8%);
  box-shadow:
    0 0.5px 1px rgb(0 0 0 / 18%),
    0 2px 6px rgb(0 0 0 / 25%),
    0 6px 16px rgb(0 0 0 / 12%),
    inset 0 0.5px 0 rgb(255 255 255 / 85%),
    inset 0 -0.5px 0 rgb(0 0 0 / 6%);
  cursor: grab;
  transition:
    box-shadow 0.4s ease,
    transform 0.15s ease;
}

input[type='range']::-webkit-slider-thumb:active {
  cursor: grabbing;
  transform: scale(0.95);
  box-shadow:
    0 0.5px 1px rgb(0 0 0 / 20%),
    0 1px 3px rgb(0 0 0 / 30%),
    0 3px 8px rgb(0 0 0 / 15%),
    inset 0 0.5px 0 rgb(255 255 255 / 70%),
    inset 0 -1px 0 rgb(0 0 0 / 8%);
}

/* 最大档：滑块泛起主题色光晕 */
input[type='range'].glowing::-webkit-slider-thumb {
  box-shadow:
    0 0.5px 1px rgb(0 0 0 / 18%),
    0 2px 6px rgb(0 0 0 / 25%),
    0 6px 16px rgb(0 0 0 / 12%),
    0 0 28px hsl(var(--primary) / 50%),
    0 0 50px hsl(var(--primary) / 25%),
    inset 0 0.5px 0 rgb(255 255 255 / 85%),
    inset 0 -0.5px 0 rgb(0 0 0 / 6%);
}

@media (prefers-reduced-motion: reduce) {
  .status-text.animate-up {
    animation: none;
  }
}
</style>
