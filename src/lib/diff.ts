/**
 * 行级 Diff（LCS 动态规划）：提示词版本对比用（§4.6）
 * 内容规模小（提示词几十行），O(m*n) 足够且实现直观
 */

export interface DiffLine {
  type: 'same' | 'added' | 'removed'
  text: string
}

export function diffLines(oldText: string, newText: string): DiffLine[] {
  const a = oldText.split('\n')
  const b = newText.split('\n')
  const m = a.length
  const n = b.length

  // dp[i][j] = a[i:] 与 b[j:] 的最长公共子序列长度
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0))
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < m && j < n) {
    if (a[i] === b[j]) {
      out.push({ type: 'same', text: a[i] })
      i++
      j++
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ type: 'removed', text: a[i] })
      i++
    } else {
      out.push({ type: 'added', text: b[j] })
      j++
    }
  }
  while (i < m) out.push({ type: 'removed', text: a[i++] })
  while (j < n) out.push({ type: 'added', text: b[j++] })
  return out
}
