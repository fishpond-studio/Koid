<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Check, Loader2, X } from 'lucide-vue-next'
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
import { proxyApi, toApiError } from '@/lib/api'
import { useProviderStore } from '@/stores/provider'
import type { Provider, ProviderType, ProxyTestResult, ProxyType } from '@/types'

/**
 * 供应商新增/编辑对话框
 * Key 处理约定：编辑时留空 = 不修改（Rust 侧仅在非空时写 keyring）
 * 代理约定：proxyType = direct 表示「跟随全局代理」（§4.4 配置粒度）
 */
const props = defineProps<{ open: boolean; provider: Provider | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()
const store = useProviderStore()

const saving = ref(false)

const form = reactive({
  name: '',
  type: 'openai-compatible' as ProviderType,
  baseUrl: '',
  apiKey: '',
  proxyType: 'direct' as ProxyType,
  proxyUrl: '',
  timeout: 60,
  retries: 2,
  enabled: true,
})

const typeOptions: ProviderType[] = [
  'openai-compatible',
  'anthropic',
  'openai-response',
  'ollama',
  'custom',
]

// 供应商级代理选项：direct 语义为跟随全局（与全局设置的「直连」区分开）
const proxyOptions: ProxyType[] = ['direct', 'http', 'socks5']

// 按类型给 Base URL 提示，降低配置出错概率
const baseUrlPlaceholder = computed(() => {
  switch (form.type) {
    case 'anthropic':
      return 'https://api.anthropic.com'
    case 'ollama':
      return 'http://localhost:11434'
    default:
      return 'https://api.openai.com/v1'
  }
})

// ---------- 连通性测试（§4.4：通过该代理请求 Base URL） ----------

const testing = ref(false)
const testResult = ref<ProxyTestResult | null>(null)

async function runTest() {
  if (testing.value || !form.baseUrl.trim()) return
  testing.value = true
  testResult.value = null
  try {
    testResult.value = await proxyApi.test({
      url: form.baseUrl.trim(),
      // direct = 走全局/环境变量，测试时如实传 direct，Rust 侧自行回退
      proxyType: form.proxyType,
      proxyUrl: form.proxyUrl.trim() || null,
      timeout: 10,
    })
  } catch (e) {
    testResult.value = {
      success: false,
      latencyMs: null,
      statusCode: null,
      error: toApiError(e).message,
    }
  } finally {
    testing.value = false
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return
    testResult.value = null
    if (props.provider) {
      form.name = props.provider.name
      form.type = props.provider.type
      form.baseUrl = props.provider.baseUrl
      form.apiKey = ''
      form.proxyType = props.provider.proxyType
      form.proxyUrl = props.provider.proxyUrl ?? ''
      form.timeout = props.provider.timeout
      form.retries = props.provider.retries
      form.enabled = props.provider.enabled
    } else {
      form.name = ''
      form.type = 'openai-compatible'
      form.baseUrl = ''
      form.apiKey = ''
      form.proxyType = 'direct'
      form.proxyUrl = ''
      form.timeout = 60
      form.retries = 2
      form.enabled = true
    }
  },
)

async function save() {
  if (!form.name.trim() || !form.baseUrl.trim()) {
    toast.error(t('settings.provider.validationRequired'))
    return
  }
  saving.value = true
  try {
    await store.save({
      id: props.provider?.id ?? null,
      name: form.name.trim(),
      type: form.type,
      baseUrl: form.baseUrl.trim().replace(/\/+$/, ''),
      apiKey: form.apiKey.trim() || null,
      proxyType: form.proxyType,
      proxyUrl: form.proxyType === 'direct' ? null : form.proxyUrl.trim() || null,
      timeout: form.timeout,
      retries: form.retries,
      enabled: form.enabled,
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
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>
          {{ provider ? t('settings.provider.edit') : t('settings.provider.add') }}
        </DialogTitle>
      </DialogHeader>

      <div class="space-y-4 py-2">
        <div class="space-y-1.5">
          <Label>{{ t('settings.provider.name') }}</Label>
          <Input v-model="form.name" :placeholder="t('settings.provider.namePlaceholder')" />
        </div>

        <div class="space-y-1.5">
          <Label>{{ t('settings.provider.type') }}</Label>
          <Select v-model="form.type">
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="opt in typeOptions" :key="opt" :value="opt">
                {{ t(`settings.provider.types.${opt}`) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="space-y-1.5">
          <Label>Base URL</Label>
          <Input v-model="form.baseUrl" :placeholder="baseUrlPlaceholder" class="font-mono text-xs" />
        </div>

        <!-- 供应商级代理：direct = 跟随全局（§4.4 配置粒度） -->
        <div class="space-y-1.5 rounded-lg border p-3">
          <Label class="text-xs">{{ t('settings.network.proxy') }}</Label>
          <div class="flex items-end gap-2">
            <div class="w-32 space-y-1">
              <Select v-model="form.proxyType">
                <SelectTrigger class="h-8 text-xs"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem v-for="pt in proxyOptions" :key="pt" :value="pt">
                    {{
                      pt === 'direct'
                        ? t('settings.network.proxyTypes.global')
                        : t(`settings.network.proxyTypes.${pt}`)
                    }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="min-w-0 flex-1">
              <Input
                v-model="form.proxyUrl"
                :disabled="form.proxyType === 'direct'"
                :placeholder="
                  form.proxyType === 'socks5'
                    ? 'socks5://127.0.0.1:1080'
                    : 'http://user:pass@127.0.0.1:7890'
                "
                class="h-8 font-mono text-xs"
              />
            </div>
            <Button
              variant="secondary"
              size="sm"
              class="h-8"
              :disabled="testing || !form.baseUrl.trim()"
              @click="() => void runTest()"
            >
              <Loader2 v-if="testing" class="mr-1 size-3.5 animate-spin" />
              {{ t('settings.network.test') }}
            </Button>
          </div>
          <p
            v-if="testResult?.success"
            class="mt-1.5 flex items-center gap-1 text-xs text-emerald-600"
          >
            <Check class="size-3" />
            {{ t('settings.network.testOk', { ms: testResult.latencyMs, code: testResult.statusCode }) }}
          </p>
          <p
            v-else-if="testResult && !testResult.success"
            class="mt-1.5 flex items-center gap-1 text-xs text-destructive"
          >
            <X class="size-3" />
            {{ testResult.error }}
          </p>
        </div>

        <div class="space-y-1.5">
          <Label>API Key</Label>
          <Input
            v-model="form.apiKey"
            type="password"
            :placeholder="
              provider ? t('settings.provider.apiKeyKeep') : t('settings.provider.apiKeyPlaceholder')
            "
            class="font-mono text-xs"
          />
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <Label>{{ t('settings.provider.timeout') }}</Label>
            <Input v-model.number="form.timeout" type="number" min="1" max="600" />
          </div>
          <div class="space-y-1.5">
            <Label>{{ t('settings.provider.retries') }}</Label>
            <Input v-model.number="form.retries" type="number" min="0" max="10" />
          </div>
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
