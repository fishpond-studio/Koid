<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import ProviderDialog from './ProviderDialog.vue'
import ModelDialog from './ModelDialog.vue'
import DiscoverModelsDialog from './DiscoverModelsDialog.vue'
import { Plus, Boxes, KeyRound, Pencil, RefreshCw, Server, Trash2 } from 'lucide-vue-next'
import { useModelStore } from '@/stores/model'
import { useProviderStore } from '@/stores/provider'
import { toApiError } from '@/lib/api'
import type { Model, Provider } from '@/types'

const { t } = useI18n()
const providers = useProviderStore()
const models = useModelStore()

const providerDialogOpen = ref(false)
const editingProvider = ref<Provider | null>(null)
const modelDialogOpen = ref(false)
const modelDialogProvider = ref<Provider | null>(null)
const editingModel = ref<Model | null>(null)
const discoverOpen = ref(false)
const discoverProvider = ref<Provider | null>(null)

function openAddProvider() {
  editingProvider.value = null
  providerDialogOpen.value = true
}

function openEditProvider(p: Provider) {
  editingProvider.value = p
  providerDialogOpen.value = true
}

function openAddModel(p: Provider) {
  modelDialogProvider.value = p
  editingModel.value = null
  modelDialogOpen.value = true
}

function openDiscover(p: Provider) {
  discoverProvider.value = p
  discoverOpen.value = true
}

function openEditModel(p: Provider, m: Model) {
  modelDialogProvider.value = p
  editingModel.value = m
  modelDialogOpen.value = true
}

async function toggleProvider(p: Provider, enabled: boolean) {
  try {
    await providers.save({ id: p.id, name: p.name, type: p.type, baseUrl: p.baseUrl, enabled })
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

async function toggleModel(m: Model, enabled: boolean) {
  try {
    await models.save({
      id: m.id,
      providerId: m.providerId,
      modelId: m.modelId,
      displayName: m.displayName,
      contextWindow: m.contextWindow,
      capabilities: m.capabilities,
      enabled,
    })
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

async function removeProvider(p: Provider) {
  if (!window.confirm(t('settings.provider.deleteConfirm', { name: p.name }))) return
  try {
    await providers.remove(p.id)
    toast.success(t('common.deleted'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

async function removeModel(m: Model) {
  if (!window.confirm(t('settings.model.deleteConfirm', { name: m.displayName }))) return
  try {
    await models.remove(m.id)
    toast.success(t('common.deleted'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}
</script>

<template>
  <div class="space-y-6 pb-16">
    <div class="flex items-center justify-between">
      <p class="text-sm text-muted-foreground">{{ t('settings.provider.subtitle') }}</p>
      <Button size="sm" class="gap-1" @click="openAddProvider">
        <Plus class="size-4" />
        {{ t('settings.provider.add') }}
      </Button>
    </div>

    <!-- 空状态 -->
    <div
      v-if="providers.providers.length === 0"
      class="flex flex-col items-center gap-2 rounded-xl border border-dashed py-16 text-muted-foreground"
    >
      <Server class="size-8" />
      <p class="text-sm">{{ t('settings.provider.empty') }}</p>
    </div>

    <!-- 供应商卡片 -->
    <Card v-for="p in providers.providers" :key="p.id">
      <CardHeader class="flex-row items-center justify-between space-y-0 px-4 py-3">
        <div class="flex items-center gap-2">
          <span class="font-medium">{{ p.name }}</span>
          <Badge variant="secondary" class="text-[10px] uppercase">{{ p.type }}</Badge>
        </div>
        <div class="flex items-center gap-1.5">
          <Switch
            :checked="p.enabled"
            :title="p.enabled ? t('common.enabled') : t('common.disabled')"
            @update:checked="(v: boolean) => toggleProvider(p, v)"
          />
          <Button variant="ghost" size="icon" class="size-8" @click="openEditProvider(p)">
            <Pencil class="size-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            class="size-8 hover:text-destructive"
            @click="removeProvider(p)"
          >
            <Trash2 class="size-3.5" />
          </Button>
        </div>
      </CardHeader>

      <CardContent class="space-y-3 px-4 pb-4">
        <div class="space-y-1 text-xs text-muted-foreground">
          <p class="truncate font-mono">{{ p.baseUrl }}</p>
          <p class="flex items-center gap-1 font-mono">
            <KeyRound class="size-3" />
            {{ p.apiKeyMasked ?? t('settings.provider.noKey') }}
            <span class="text-muted-foreground/60">· {{ p.timeout }}s ·</span>
            {{ t('settings.provider.retries') }} {{ p.retries }}
          </p>
        </div>

        <!-- 模型列表 -->
        <div class="rounded-lg border">
          <div class="flex items-center justify-between border-b px-3 py-2">
            <span class="flex items-center gap-1.5 text-xs font-medium">
              <Boxes class="size-3.5" />
              {{ t('settings.model.title') }}
            </span>
            <span class="flex items-center gap-1">
              <!-- 模型发现（§4.2）：/v1/models 拉取 + 勾选启用 -->
              <Button
                variant="outline"
                size="sm"
                class="h-6 gap-1 text-xs"
                :disabled="!p.enabled"
                :title="p.enabled ? '' : t('settings.model.discoverDisabledHint')"
                @click="openDiscover(p)"
              >
                <RefreshCw class="size-3" />
                {{ t('settings.model.discover') }}
              </Button>
              <Button variant="ghost" size="sm" class="h-6 gap-1 text-xs" @click="openAddModel(p)">
                <Plus class="size-3" />
                {{ t('settings.model.add') }}
              </Button>
            </span>
          </div>

          <button
            v-if="models.byProvider(p.id).length === 0"
            class="flex w-full items-center gap-2 px-3 py-4 text-xs text-muted-foreground transition-colors hover:bg-secondary/40 hover:text-foreground"
            @click="openDiscover(p)"
          >
            <RefreshCw class="size-3.5" />
            {{ t('settings.model.empty') }} · {{ t('settings.model.discover') }}
          </button>
          <div
            v-for="m in models.byProvider(p.id)"
            :key="m.id"
            class="group flex items-center gap-2 border-b px-3 py-2 last:border-b-0"
          >
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm">{{ m.displayName }}</p>
              <p class="truncate font-mono text-[10px] text-muted-foreground">
                {{ m.modelId }}
                <template v-if="m.contextWindow"> · {{ Math.round(m.contextWindow / 1000) }}k</template>
              </p>
            </div>
            <Button
              variant="ghost"
              size="icon"
              class="size-7 opacity-0 group-hover:opacity-100"
              @click="openEditModel(p, m)"
            >
              <Pencil class="size-3" />
            </Button>
            <Switch
              :checked="m.enabled"
              class="scale-90"
              @update:checked="(v: boolean) => toggleModel(m, v)"
            />
            <Button
              variant="ghost"
              size="icon"
              class="size-7 opacity-0 hover:text-destructive group-hover:opacity-100"
              @click="removeModel(m)"
            >
              <Trash2 class="size-3" />
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>

    <!-- 对话框 -->
    <ProviderDialog v-model:open="providerDialogOpen" :provider="editingProvider" />
    <ModelDialog
      v-model:open="modelDialogOpen"
      :provider="modelDialogProvider"
      :model="editingModel"
    />
    <DiscoverModelsDialog v-model:open="discoverOpen" :provider="discoverProvider" />
  </div>
</template>
