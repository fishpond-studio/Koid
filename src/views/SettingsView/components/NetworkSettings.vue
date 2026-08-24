<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { ArrowDown, ArrowUp, Check, Loader2, Plus, X } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { Switch } from '@/components/ui/switch'
import { proxyApi, toApiError } from '@/lib/api'
import { useSettingsStore } from '@/stores/settings'
import { useModelStore } from '@/stores/model'
import { useProviderStore } from '@/stores/provider'
import type { FailoverStrategy, FailoverTrigger, ProxyTestResult, ProxyType } from '@/types'

const { t } = useI18n()
const settings = useSettingsStore()
const models = useModelStore()
const providers = useProviderStore()

// ---------- 全局代理 ----------

const testTarget = ref('https://api.openai.com/v1')
const testing = ref(false)
const testResult = ref<ProxyTestResult | null>(null)

const proxyTypes: ProxyType[] = ['direct', 'http', 'socks5']
const strategies: FailoverStrategy[] = ['sequential', 'round-robin', 'random']
const triggers: FailoverTrigger[] = ['timeout', '5xx', 'empty-response', 'content-filter']

async function runTest() {
  if (testing.value) return
  testing.value = true
  testResult.value = null
  try {
    testResult.value = await proxyApi.test({
      url: testTarget.value.trim(),
      proxyType: settings.globalProxy.proxyType,
      proxyUrl: settings.globalProxy.proxyUrl,
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

async function saveProxy() {
  // direct 模式下清空 URL，避免残留配置造成误解
  if (settings.globalProxy.proxyType === 'direct') {
    settings.globalProxy.proxyUrl = null
  }
  try {
    await settings.saveGlobalProxy()
    toast.success(t('common.saved'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- 故障转移 ----------

function toggleTrigger(tr: FailoverTrigger) {
  const list = settings.failover.triggerConditions
  const idx = list.indexOf(tr)
  if (idx >= 0) list.splice(idx, 1)
  else list.push(tr)
}

async function saveFailover() {
  try {
    await settings.saveFailover()
    toast.success(t('common.saved'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- 备选模型链（用户自选，按序接管；可跨/同供应商） ----------

/** 待添加的模型 id */
const pendingModelId = ref<string | null>(null)

/** 可选模型：全部启用的模型（显示「供应商 / 模型名」），排除已在链中的 */
const availableModels = computed(() =>
  models.enabledModels
    .filter((m) => !settings.failover.fallbackChain.includes(m.id))
    .map((m) => ({
      id: m.id,
      label: `${providers.get(m.providerId)?.name ?? '?'} / ${m.displayName}`,
    })),
)

/** 链中项的展示名 */
function chainLabel(id: string): string {
  const m = models.get(id)
  if (!m) return id
  return `${providers.get(m.providerId)?.name ?? '?'} / ${m.displayName}`
}

function addChainModel() {
  if (!pendingModelId.value) return
  if (!settings.failover.fallbackChain.includes(pendingModelId.value)) {
    settings.failover.fallbackChain.push(pendingModelId.value)
  }
  pendingModelId.value = null
}

function removeChainModel(idx: number) {
  settings.failover.fallbackChain.splice(idx, 1)
}

function moveChainModel(idx: number, dir: -1 | 1) {
  const list = settings.failover.fallbackChain
  const next = idx + dir
  if (next < 0 || next >= list.length) return
  ;[list[idx], list[next]] = [list[next], list[idx]]
}
</script>

<template>
  <div class="space-y-8 pb-16">
    <!-- 全局代理 -->
    <section class="space-y-3">
      <div>
        <Label>{{ t('settings.network.globalProxy') }}</Label>
        <p class="mt-0.5 text-xs text-muted-foreground">{{ t('settings.network.proxyHint') }}</p>
      </div>

      <div class="flex items-end gap-3">
        <div class="w-36 space-y-1.5">
          <Label class="text-xs">{{ t('settings.network.type') }}</Label>
          <Select v-model="settings.globalProxy.proxyType">
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="pt in proxyTypes" :key="pt" :value="pt">
                {{ t(`settings.network.proxyTypes.${pt}`) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="min-w-0 flex-1 space-y-1.5">
          <Label class="text-xs">{{ t('settings.network.proxyUrl') }}</Label>
          <Input
            :model-value="settings.globalProxy.proxyUrl ?? ''"
            :disabled="settings.globalProxy.proxyType === 'direct'"
            :placeholder="
              settings.globalProxy.proxyType === 'socks5'
                ? 'socks5://127.0.0.1:1080'
                : 'http://user:pass@127.0.0.1:7890'
            "
            class="font-mono text-xs"
            @update:model-value="(v: string | number) => (settings.globalProxy.proxyUrl = String(v))"
          />
        </div>

        <Button variant="outline" @click="saveProxy">{{ t('common.save') }}</Button>
      </div>

      <!-- 连通性测试（§4.4） -->
      <div class="rounded-lg border p-3">
        <div class="flex items-end gap-3">
          <div class="min-w-0 flex-1 space-y-1.5">
            <Label class="text-xs">{{ t('settings.network.testTarget') }}</Label>
            <Input v-model="testTarget" class="font-mono text-xs" />
          </div>
          <Button variant="secondary" :disabled="testing" @click="() => void runTest()">
            <Loader2 v-if="testing" class="mr-1 size-4 animate-spin" />
            {{ t('settings.network.test') }}
          </Button>
        </div>

        <p v-if="testResult?.success" class="mt-2 flex items-center gap-1 text-xs text-emerald-600">
          <Check class="size-3.5" />
          {{ t('settings.network.testOk', { ms: testResult.latencyMs, code: testResult.statusCode }) }}
        </p>
        <p v-else-if="testResult && !testResult.success" class="mt-2 flex items-center gap-1 text-xs text-destructive">
          <X class="size-3.5" />
          {{ testResult.error }}
        </p>
      </div>
    </section>

    <Separator />

    <!-- 故障转移 -->
    <section class="space-y-4">
      <div class="flex items-center justify-between">
        <div>
          <Label>{{ t('settings.network.failover') }}</Label>
          <p class="mt-0.5 text-xs text-muted-foreground">
            {{ t('settings.network.failoverHint') }}
          </p>
        </div>
        <Switch v-model:checked="settings.failover.enabled" />
      </div>

      <div v-if="settings.failover.enabled" class="space-y-4 rounded-lg border p-3">
        <div class="flex items-end gap-3">
          <div class="w-44 space-y-1.5">
            <Label class="text-xs">{{ t('settings.network.strategy') }}</Label>
            <Select v-model="settings.failover.strategy">
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="s in strategies" :key="s" :value="s">
                  {{ t(`settings.network.strategies.${s}`) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div class="w-40 space-y-1.5">
            <Label class="text-xs">{{ t('settings.network.maxBackoff') }}</Label>
            <Input
              v-model.number="settings.failover.maxBackoffSeconds"
              type="number"
              min="1"
              max="120"
            />
          </div>

          <Button variant="outline" class="ml-auto" @click="saveFailover">
            {{ t('common.save') }}
          </Button>
        </div>

        <div class="space-y-1.5">
          <Label class="text-xs">{{ t('settings.network.triggers') }}</Label>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="tr in triggers"
              :key="tr"
              class="rounded-full border px-3 py-1 text-xs transition-colors"
              :class="
                settings.failover.triggerConditions.includes(tr)
                  ? 'border-primary bg-primary/10 text-primary'
                  : 'text-muted-foreground hover:bg-secondary'
              "
              @click="toggleTrigger(tr)"
            >
              {{ t(`settings.network.triggerLabels.${tr}`) }}
            </button>
          </div>
        </div>

        <!-- 备选模型链（用户自选，按序接管） -->
        <div class="space-y-2">
          <Label class="text-xs">{{ t('settings.network.fallbackChain') }}</Label>
          <p class="text-xs text-muted-foreground">
            {{ t('settings.network.fallbackChainHint') }}
          </p>

          <!-- 已配置的链 -->
          <div v-if="settings.failover.fallbackChain.length" class="space-y-1">
            <div
              v-for="(id, idx) in settings.failover.fallbackChain"
              :key="id"
              class="flex items-center gap-2 rounded-md border bg-background/50 px-2.5 py-1.5"
            >
              <span class="w-5 shrink-0 text-center font-mono text-[10px] text-muted-foreground">
                {{ idx + 1 }}
              </span>
              <span class="min-w-0 flex-1 truncate text-xs">{{ chainLabel(id) }}</span>
              <button
                class="rounded p-1 text-muted-foreground transition-colors hover:bg-secondary disabled:opacity-30"
                :disabled="idx === 0"
                :title="t('common.moveUp')"
                @click="moveChainModel(idx, -1)"
              >
                <ArrowUp class="size-3.5" />
              </button>
              <button
                class="rounded p-1 text-muted-foreground transition-colors hover:bg-secondary disabled:opacity-30"
                :disabled="idx === settings.failover.fallbackChain.length - 1"
                :title="t('common.moveDown')"
                @click="moveChainModel(idx, 1)"
              >
                <ArrowDown class="size-3.5" />
              </button>
              <button
                class="rounded p-1 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                :title="t('common.delete')"
                @click="removeChainModel(idx)"
              >
                <X class="size-3.5" />
              </button>
            </div>
          </div>

          <!-- 添加模型 -->
          <div class="flex items-center gap-2">
            <Select v-model="pendingModelId">
              <SelectTrigger class="min-w-0 flex-1">
                <SelectValue :placeholder="t('settings.network.pickFallbackModel')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="m in availableModels" :key="m.id" :value="m.id">
                  {{ m.label }}
                </SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              size="sm"
              class="shrink-0 gap-1"
              :disabled="!pendingModelId"
              @click="addChainModel"
            >
              <Plus class="size-3.5" />
              {{ t('common.add') }}
            </Button>
          </div>

          <p class="text-xs text-muted-foreground/70">
            {{ t('settings.network.fallbackChainEmptyHint') }}
          </p>
        </div>

        <p class="text-xs text-muted-foreground">
          {{ t('settings.network.excludedHint') }}
        </p>
      </div>
    </section>
  </div>
</template>
