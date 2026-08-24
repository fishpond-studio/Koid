<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { usePromptStore } from '@/stores/prompts'
import { toApiError } from '@/lib/api'
import type { Prompt, PromptType } from '@/types'

/** 与 Rust parse_variables 相同的提取规则：{{var}}，保持顺序去重 */
function parseVars(content: string): string[] {
  const out: string[] = []
  const re = /\{\{\s*([A-Za-z0-9_-]+)\s*\}\}/g
  let m: RegExpExecArray | null
  while ((m = re.exec(content))) {
    if (!out.includes(m[1])) out.push(m[1])
  }
  return out
}

const props = defineProps<{ open: boolean; prompt: Prompt | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()
const store = usePromptStore()

const saving = ref(false)
const form = reactive({
  title: '',
  type: 'template' as PromptType,
  content: '',
  tags: '',
})

const typeOptions: PromptType[] = ['template', 'snippet', 'system']
const vars = computed(() => parseVars(form.content))

// 模板内不能直接写字面 {{，用函数包裹展示变量标记
function fmtVar(v: string): string {
  return '{{' + v + '}}'
}

watch(
  () => props.open,
  (open) => {
    if (!open) return
    if (props.prompt) {
      form.title = props.prompt.title
      form.type = props.prompt.type
      form.content = props.prompt.content
      form.tags = props.prompt.tags.filter((x) => x !== '内置').join(', ')
    } else {
      form.title = ''
      form.type = 'template'
      form.content = ''
      form.tags = ''
    }
  },
)

async function save() {
  if (!form.title.trim() || !form.content.trim()) {
    toast.error(t('settings.prompts.validationRequired'))
    return
  }
  saving.value = true
  try {
    const tags = form.tags
      .split(/[,，]/)
      .map((x) => x.trim())
      .filter(Boolean)
    await store.save({
      id: props.prompt?.id ?? null,
      title: form.title.trim(),
      content: form.content,
      type: form.type,
      tags,
    })
    toast.success(t('common.saved'))
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
    <DialogContent class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>
          {{ prompt ? t('settings.prompts.edit') : t('settings.prompts.add') }}
        </DialogTitle>
      </DialogHeader>

      <div class="space-y-4 py-2">
        <div class="flex gap-3">
          <div class="min-w-0 flex-1 space-y-1.5">
            <Label>{{ t('settings.prompts.titleField') }}</Label>
            <Input v-model="form.title" :placeholder="t('settings.prompts.titlePlaceholder')" />
          </div>
          <div class="w-36 space-y-1.5">
            <Label>{{ t('settings.prompts.typeField') }}</Label>
            <Select v-model="form.type">
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="opt in typeOptions" :key="opt" :value="opt">
                  {{ t(`settings.prompts.types.${opt}`) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div class="space-y-1.5">
          <Label>{{ t('settings.prompts.contentField') }}</Label>
          <textarea
            v-model="form.content"
            rows="8"
            class="w-full resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-2 focus:ring-ring"
            :placeholder="t('settings.prompts.contentPlaceholder')"
          />
          <!-- 变量实时预览：与 Rust 解析规则一致 -->
          <p v-if="vars.length" class="flex flex-wrap items-center gap-1 text-xs">
            <span class="text-muted-foreground">{{ t('settings.prompts.detectedVars') }}:</span>
            <Badge v-for="v in vars" :key="v" variant="secondary" class="font-mono">
              {{ fmtVar(v) }}
            </Badge>
          </p>
        </div>

        <div class="space-y-1.5">
          <Label>{{ t('settings.prompts.tagsField') }}</Label>
          <Input v-model="form.tags" :placeholder="t('settings.prompts.tagsPlaceholder')" />
        </div>
      </div>

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
