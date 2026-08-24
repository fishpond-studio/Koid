import { ref } from 'vue'
import { defineStore } from 'pinia'
import { skillsApi, toApiError } from '@/lib/api'
import type { SkillDef } from '@/types'

/**
 * Skills Store（§4.7）
 * 内置 Skill 编译期嵌入 Rust 层；用户 Skill 存于应用数据目录
 */
export const useSkillStore = defineStore('skill', () => {
  const skills = ref<SkillDef[]>([])
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      skills.value = await skillsApi.list()
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function save(content: string): Promise<SkillDef> {
    try {
      const saved = await skillsApi.save(content)
      await load(true)
      return saved
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function remove(id: string) {
    try {
      await skillsApi.remove(id)
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  return { skills, loaded, load, save, remove }
})
