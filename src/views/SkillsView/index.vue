<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import {
  Bug,
  GitPullRequest,
  Play,
  Plus,
  Pencil,
  ShieldAlert,
  Sparkles,
  Trash2,
} from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import SkillRunDialog from './components/SkillRunDialog.vue'
import SkillEditorDialog from './components/SkillEditorDialog.vue'
import { useSkillStore } from '@/stores/skills'
import { skillsApi, toApiError } from '@/lib/api'
import type { SkillDef } from '@/types'

const { t } = useI18n()
const store = useSkillStore()

onMounted(async () => {
  try {
    await store.load()
  } catch (e) {
    toast.error(toApiError(e).message)
  }
})

const runSkill = ref<SkillDef | null>(null)
const runOpen = ref(false)
const trustSkill = ref<SkillDef | null>(null)
const trustOpen = ref(false)
const editSkill = ref<SkillDef | null>(null)
const editOpen = ref(false)

/** 图标名 → lucide 组件映射（缺省 Sparkles） */
const iconMap: Record<string, typeof Sparkles> = {
  'git-pull-request': GitPullRequest,
  bug: Bug,
}

function iconOf(s: SkillDef) {
  return (s.icon && iconMap[s.icon]) || Sparkles
}

function openRun(s: SkillDef) {
  // 默认禁用的全局技能：先展示其完整正文，确认可信后才持久化启用
  if (s.source === 'global' && !s.enabled) {
    trustSkill.value = s
    trustOpen.value = true
    return
  }
  runSkill.value = s
  runOpen.value = true
}

async function confirmTrust() {
  if (!trustSkill.value) return
  try {
    await skillsApi.enableGlobal(trustSkill.value.id)
    await store.load()
    trustOpen.value = false
    trustSkill.value = null
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

function openAdd() {
  editSkill.value = null
  editOpen.value = true
}

function openEdit(s: SkillDef) {
  editSkill.value = s
  editOpen.value = true
}

async function remove(s: SkillDef) {
  if (!window.confirm(t('skills.deleteConfirm', { name: s.name }))) return
  try {
    await store.remove(s.id)
    toast.success(t('common.deleted'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

const sorted = computed(() =>
  [...store.skills].sort((a, b) => (a.source === 'builtin' ? -1 : 1) - (b.source === 'builtin' ? -1 : 1)),
)

/** 审阅框展示内容：优先 SKILL.md 正文；旧版 YAML 无正文时回退展示真正注入模型的步骤内容 */
const trustContent = computed(() => {
  const s = trustSkill.value
  if (!s) return ''
  if (s.body && s.body.trim()) return s.body
  return s.steps
    .map((st) => st.prompt || st.content || '')
    .filter((x) => x.trim())
    .join('\n\n')
})
</script>

<template>
  <div class="mx-auto h-full max-w-5xl overflow-y-auto p-8">
    <div class="mb-6 flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t('skills.title') }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('skills.subtitle') }}</p>
      </div>
      <Button class="gap-1" @click="openAdd">
        <Plus class="size-4" />
        {{ t('skills.add') }}
      </Button>
    </div>

    <p
      v-if="sorted.length === 0"
      class="py-24 text-center text-sm text-muted-foreground"
    >
      {{ t('skills.empty') }}
    </p>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <div
        v-for="s in sorted"
        :key="s.id"
        class="group flex flex-col rounded-xl border bg-card p-4 transition-shadow hover:shadow-md"
      >
        <div class="flex items-start gap-3">
          <div class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10">
            <component :is="iconOf(s)" class="size-5 text-primary" />
          </div>
          <div class="min-w-0 flex-1">
            <p class="truncate font-medium">{{ s.name }}</p>
            <div class="mt-0.5 flex items-center gap-1.5">
              <Badge
                :variant="s.source === 'builtin' ? 'secondary' : 'outline'"
                class="text-[10px]"
              >
                {{ s.source === 'builtin' ? t('skills.builtin') : (s.source === 'global' ? t('skills.global') : t('skills.user')) }}
              </Badge>
              <span class="text-[10px] text-muted-foreground">
                {{ s.steps.length }} {{ t('skills.steps') }}
              </span>
            </div>
          </div>
        </div>

        <p class="mt-3 line-clamp-2 flex-1 text-xs text-muted-foreground">
          {{ s.description }}
        </p>

        <div class="mt-4 flex items-center gap-1">
          <Button size="sm" class="gap-1" @click="openRun(s)">
            <Play class="size-3.5" />
            {{ t('skills.run') }}
          </Button>
          <div class="ml-auto flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
            <Button
              v-if="s.source === 'user' && s.body != null"
              variant="ghost"
              size="icon"
              class="size-7"
              @click="openEdit(s)"
            >
              <Pencil class="size-3.5" />
            </Button>
            <Button
              v-if="s.source === 'user'"
              variant="ghost"
              size="icon"
              class="size-7 hover:text-destructive"
              @click="() => void remove(s)"
            >
              <Trash2 class="size-3.5" />
            </Button>
          </div>
        </div>
      </div>
    </div>

    <SkillRunDialog v-model:open="runOpen" :skill="runSkill" />
    <SkillEditorDialog v-model:open="editOpen" :skill="editSkill" @saved="() => {}" />

    <!-- 全局技能启用确认：展示完整正文，用户审阅后持久化启用 -->
    <Dialog :open="trustOpen" @update:open="(v: boolean) => (trustOpen = v)">
      <DialogContent class="flex max-h-[80vh] max-w-2xl flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle class="flex items-center gap-2">
            <ShieldAlert class="size-4 text-destructive" />
            {{ t('skills.enableGlobalTitle') }} · {{ trustSkill?.name }}
          </DialogTitle>
        </DialogHeader>
        <div class="min-h-0 flex-1 space-y-3 overflow-y-auto py-1">
          <p class="text-sm text-muted-foreground">{{ t('skills.globalTrustHint') }}</p>
          <pre
            class="whitespace-pre-wrap rounded-lg border bg-muted/30 px-3 py-2 font-mono text-xs leading-5"
          >{{ trustContent || t('skills.empty') }}</pre>
        </div>
        <DialogFooter class="pt-3">
          <Button variant="outline" @click="trustOpen = false">
            {{ t('common.cancel') }}
          </Button>
          <Button @click="() => void confirmTrust()">
            {{ t('skills.enableGlobal') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
