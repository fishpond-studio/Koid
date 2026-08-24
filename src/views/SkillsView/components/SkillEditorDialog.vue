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

/** Skill YAML 编辑器：新建/编辑（§4.7 双模式编辑，源码优先） */
const props = defineProps<{ open: boolean; skill: SkillDef | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean]; saved: [] }>()

const { t } = useI18n()
const store = useSkillStore()

const content = ref('')
const saving = ref(false)

const STARTER = `# Skill 定义示例
id: my-skill
name: My Skill
description: What this skill does
# model: gpt-4o            # 可选：按 model_id / displayName 指定
# systemPrompt: You are... # 可选：步骤通用 system prompt
steps:
  - id: ask
    type: input
    content: What input do you need?

  - id: respond
    type: llm
    prompt: |
      Process the following input:

      {{ask.output}}

  - id: done
    type: message
    content: "{{respond.output}}"
`

watch(
  () => props.open,
  (open) => {
    if (!open) return
    content.value = props.skill
      ? serializeYaml(props.skill)
      : STARTER
  },
)

/** 把 SkillDef 序列化为 YAML（前端轻量实现；编辑以文本为准，保存时后端再校验） */
function serializeYaml(s: SkillDef): string {
  const lines: string[] = [
    `id: ${s.id}`,
    `name: ${s.name}`,
    `description: ${s.description}`,
  ]
  if (s.icon) lines.push(`icon: ${s.icon}`)
  if (s.model) lines.push(`model: ${s.model}`)
  if (s.systemPrompt) lines.push(`systemPrompt: |\n  ${s.systemPrompt.replaceAll('\n', '\n  ')}`)
  lines.push('steps:')
  for (const step of s.steps) {
    lines.push(`  - id: ${step.id}`)
    lines.push(`    type: ${step.type}`)
    const fields: [string, string | null | undefined][] = [
      ['prompt', step.prompt],
      ['content', step.content],
      ['condition', step.condition],
      ['then', step.then],
      ['else', step.else],
      ['tool', step.tool],
      ['server', step.server],
      ['args', step.args],
    ]
    for (const [k, v] of fields) {
      if (v === undefined || v === null) continue
      lines.push(`    ${k}: |\n${v.split('\n').map((l) => `      ${l}`).join('\n')}`)
    }
  }
  return lines.join('\n') + '\n'
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
