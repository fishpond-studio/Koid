/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  // eslint 场景下的宽松声明：单文件组件类型由 vue-tsc 精确推导
  const component: DefineComponent<object, object, unknown>
  export default component
}
