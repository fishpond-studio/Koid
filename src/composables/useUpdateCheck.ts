import { getVersion } from '@tauri-apps/api/app'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'

/** 仓库 Releases 页（点击 Toast 跳转） */
const RELEASES_URL = 'https://github.com/fishpond-studio/Koid/releases/latest'
/** GitHub Releases 最新版查询 */
const RELEASE_API = 'https://api.github.com/repos/fishpond-studio/Koid/releases/latest'

/** 语义化版本比较：>0 表示 a 更新；忽略 v 前缀与预发布段 */
function compareVersions(a: string, b: string): number {
  const pa = a.replace(/^v/i, '').split(/[.\-+]/).map((x) => parseInt(x, 10) || 0)
  const pb = b.replace(/^v/i, '').split(/[.\-+]/).map((x) => parseInt(x, 10) || 0)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0)
    if (d !== 0) return d
  }
  return 0
}

/**
 * 更新检查：启动后查询 GitHub Releases 最新版本，
 * 有新版时 Toast 提示并提供「前往下载」（打开系统浏览器）。
 * 无网络 / 未发布 Release / 请求失败均静默，不打扰使用。
 */
export function useUpdateCheck() {
  const { t } = useI18n()

  async function check() {
    try {
      const [current, res] = await Promise.all([
        getVersion(),
        fetch(RELEASE_API, { headers: { Accept: 'application/vnd.github+json' } }),
      ])
      if (!res.ok) return
      const data: { tag_name?: string } = await res.json()
      const latest = data.tag_name
      if (!latest || compareVersions(latest, current) <= 0) return
      toast.info(t('update.available', { version: latest.replace(/^v/i, '') }), {
        description: t('update.availableHint'),
        action: {
          label: t('update.download'),
          onClick: () => void openUrl(RELEASES_URL),
        },
        duration: 10000,
      })
    } catch {
      /* 离线 / 限流：静默跳过 */
    }
  }

  return { check }
}
