<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useSkillStore } from '@/stores/skills'
import { toApiError } from '@/lib/api'
import type { SkillDef } from '@/types'

/** Skill 编辑器：新建/编辑（标准 SKILL.md 源码优先） */
const props = defineProps<{ open: boolean; skill: SkillDef | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean]; saved: [] }>()

const { t } = useI18n()
const store = useSkillStore()

const content = ref('')
const saving = ref(false)

// 标准 Agent SKILL.md 模板（YAML Frontmatter + Markdown Body）
const STARTER = `---
id: my-skill
name: My Skill
description: What this skill does
# icon: sparkles           # 可选：图标名
# model: gpt-4o            # 可选：按 model_id / displayName 指定
# system_prompt: You are…  # 可选：该技能的 system prompt
---

# 任务说明

在这里用 Markdown 描述技能的具体任务与要求。
正文会与用户输入一起作为提示词发送给模型。
`

watch(
  () => props.open,
  (open) => {
    if (!open) return
    content.value = props.skill ? serializeSkillMd(props.skill) : STARTER
  },
)

/** 把 SkillDef 序列化为标准 SKILL.md（前端轻量实现；编辑以文本为准，保存时后端再校验） */
function serializeSkillMd(s: SkillDef): string {
  const fm: string[] = ['---', `id: ${s.id}`, `name: ${s.name}`]
  if (s.description) fm.push(`description: ${s.description}`)
  if (s.icon) fm.push(`icon: ${s.icon}`)
  if (s.model) fm.push(`model: ${s.model}`)
  if (s.systemPrompt) {
    fm.push(`system_prompt: |\n  ${s.systemPrompt.replaceAll('\n', '\n  ')}`)
  }
  fm.push('---', '')
  return fm.join('\n') + (s.body ?? '') + '\n'
}

async function save() {
  saving.value = true
  try {
    await store.save(content.value)
    toast.success(t('common.saved'))
    emit('saved')
    emit('update:open', false)
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => emit('update:open', v)">
    <DialogContent class="max-w-3xl">
      <DialogHeader>
        <DialogTitle>
          {{ skill ? t('skills.edit') : t('skills.add') }}
        </DialogTitle>
      </DialogHeader>

      <textarea
        v-model="content"
        rows="18"
        spellcheck="false"
        class="w-full resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs leading-5 outline-none focus:ring-2 focus:ring-ring"
        :placeholder="t('skills.yamlPlaceholder')"
      />

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">
          {{ t('common.cancel') }}
        </Button>
        <Button :disabled="saving" @click="() => void save()">
          {{ t('common.save') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
