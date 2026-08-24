import { createApp } from 'vue'
import { createPinia } from 'pinia'
import piniaPluginPersistedstate from 'pinia-plugin-persistedstate'
import { MotionPlugin } from '@vueuse/motion'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import 'vue-sonner/style.css'
import './styles/globals.css'

const app = createApp(App)

const pinia = createPinia()
// 持久化插件：主题等 store 落 localStorage（计划 §二）
pinia.use(piniaPluginPersistedstate)

app.use(pinia)
app.use(router)
app.use(i18n)
app.use(MotionPlugin)

app.mount('#app')
