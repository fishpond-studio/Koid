<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { Download, Upload } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { backupApi, toApiError } from '@/lib/api'

/**
 * 加密备份（§4.11 云端同步基础）
 * 导出：口令 → AES-256-GCM 加密 → .koid-backup 文件（E2E）
 * 导入：解密后合并进当前数据库，随后前端刷新各 store
 */
const { t } = useI18n()

const passphrase = ref('')
const busy = ref(false)

async function doExport() {
  if (!passphrase.value) {
    toast.error(t('settings.backup.passphraseRequired'))
    return
  }
  const dest = await saveDialog({
    defaultPath: 'koid-backup.koid-backup',
    filters: [{ name: 'Koid Backup', extensions: ['koid-backup'] }],
  })
  if (!dest) return
  busy.value = true
  try {
    const path = await backupApi.export(passphrase.value, String(dest))
    toast.success(t('settings.backup.exportDone', { path }))
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    busy.value = false
  }
}

async function doImport() {
  if (!passphrase.value) {
    toast.error(t('settings.backup.passphraseRequired'))
    return
  }
  const picked = await openDialog({
    multiple: false,
    filters: [{ name: 'Koid Backup', extensions: ['koid-backup'] }],
  })
  if (!picked) return
  busy.value = true
  try {
    await backupApi.import(passphrase.value, String(picked))
    toast.success(t('settings.backup.importDone'))
    // 数据已变更：延迟刷新让 toast 先呈现
    window.setTimeout(() => window.location.reload(), 800)
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="space-y-6 pb-16">
    <div>
      <Label>{{ t('settings.backup.passphrase') }}</Label>
      <p class="mt-0.5 text-xs text-muted-foreground">{{ t('settings.backup.passphraseHint') }}</p>
    </div>

    <Input
      v-model="passphrase"
      type="password"
      :placeholder="t('settings.backup.passphrasePlaceholder')"
      class="max-w-sm"
    />

    <div class="flex gap-2">
      <Button :disabled="busy" class="gap-1" @click="() => void doExport()">
        <Download class="size-4" />
        {{ t('settings.backup.export') }}
      </Button>
      <Button :disabled="busy" variant="outline" class="gap-1" @click="() => void doImport()">
        <Upload class="size-4" />
        {{ t('settings.backup.import') }}
      </Button>
    </div>
  </div>
</template>
