<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import {
  SwitchRoot,
  SwitchThumb,
} from "reka-ui"
import { cn } from "@/lib/utils"

/**
 * shadcn-vue 标准 Switch：对外暴露 checked / update:checked（v-model:checked），
 * 内部映射到 reka-ui 的 modelValue / update:modelValue
 */
const props = defineProps<{
  checked?: boolean
  defaultChecked?: boolean
  disabled?: boolean
  id?: string
  class?: HTMLAttributes["class"]
}>()

const emit = defineEmits<{ 'update:checked': [value: boolean] }>()
</script>

<template>
  <SwitchRoot
    :model-value="checked"
    :default-value="defaultChecked"
    :disabled="disabled"
    :id="id"
    @update:model-value="(v: boolean) => emit('update:checked', v)"
    :class="cn(
      'peer inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-primary data-[state=unchecked]:bg-input',
      props.class,
    )"
  >
    <SwitchThumb
      :class="cn('pointer-events-none block h-5 w-5 rounded-full bg-background shadow-lg ring-0 transition-transform data-[state=checked]:translate-x-5')"
    >
      <slot name="thumb" />
    </SwitchThumb>
  </SwitchRoot>
</template>
