import { ref, watch, onMounted, onBeforeUnmount, nextTick, type Ref } from 'vue'
import { VERT, FRAG_SIM, FRAG_BLUR, FRAG_COMP } from './shaders'

/** 火焰三档配色（0..1 线性 RGB）：深色基底 / 主题主色 / 白热核心 */
export interface FirePalette {
  deep: [number, number, number]
  main: [number, number, number]
  hot: [number, number, number]
}

/** hsl(h∈[0,360), s/l∈[0,1]) → rgb ∈ [0,1] */
function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s
  const hp = (((h % 360) + 360) % 360) / 60
  const x = c * (1 - Math.abs((hp % 2) - 1))
  let r = 0
  let g = 0
  let b = 0
  if (hp < 1) [r, g, b] = [c, x, 0]
  else if (hp < 2) [r, g, b] = [x, c, 0]
  else if (hp < 3) [r, g, b] = [0, c, x]
  else if (hp < 4) [r, g, b] = [0, x, c]
  else if (hp < 5) [r, g, b] = [x, 0, c]
  else [r, g, b] = [c, 0, x]
  const m = l - c / 2
  return [r + m, g + m, b + m]
}

/** 归一化到指定峰值亮度（对齐原版配色：主色峰值为 1.0） */
function scaleTo(c: [number, number, number], peak: number): [number, number, number] {
  const m = Math.max(c[0], c[1], c[2]) || 1
  return [(c[0] / m) * peak, (c[1] / m) * peak, (c[2] / m) * peak]
}

/** 从当前主题读取 --primary（形如 "239 84% 67%"），派生火焰三色 */
export function themeFirePalette(): FirePalette {
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--primary').trim()
  const parts = raw.split(/[\s,]+/).map((v) => parseFloat(v))
  const h = Number.isFinite(parts[0]) ? parts[0] : 239
  const s = Number.isFinite(parts[1]) ? parts[1] / 100 : 0.84
  const l = Number.isFinite(parts[2]) ? parts[2] / 100 : 0.67
  return {
    deep: scaleTo(hslToRgb(h, Math.min(1, s * 0.85), Math.max(0.1, l * 0.42)), 0.6),
    main: scaleTo(hslToRgb(h, s, Math.min(0.8, l)), 1.0),
    hot: hslToRgb(h, s * 0.25, Math.min(0.98, l + 0.32)),
  }
}

/**
 * WebGL2 渲染引擎（移植自参考实现）：
 * 4-pass 管线（模拟 → 横向模糊 → 纵向模糊 → 合成），闲置自动停帧，
 * 尺寸变化重建 FBO，context 丢失恢复，资源卸载全量释放。
 */
export function useWebglFire(
  canvasRef: Ref<HTMLCanvasElement | null>,
  sliderValue: Ref<number>,
  isActive: Ref<boolean>,
) {
  let gl: WebGL2RenderingContext | null = null
  let canvasEl: HTMLCanvasElement | null = null
  let rafId: number | null = null
  let resizeObserver: ResizeObserver | null = null
  let resizeDebounce: ReturnType<typeof setTimeout> | null = null

  let loopRunning = false
  let idleFrames = 0
  let wasActive = false
  let ultraStart: number | null = null

  const MAX_IDLE = 180

  let simProg: WebGLProgram | null = null
  let blurProg: WebGLProgram | null = null
  let compProg: WebGLProgram | null = null
  let vao: WebGLVertexArrayObject | null = null
  let vbo: WebGLBuffer | null = null
  let programsReady = false

  type FBO = { fbo: WebGLFramebuffer; tex: WebGLTexture }
  let simA: FBO | null = null
  let simB: FBO | null = null
  let blurH: FBO | null = null
  let blurV: FBO | null = null

  const U = {
    simTime: null as WebGLUniformLocation | null,
    simSlider: null as WebGLUniformLocation | null,
    simElapsed: null as WebGLUniformLocation | null,
    simBack: null as WebGLUniformLocation | null,
    simDeep: null as WebGLUniformLocation | null,
    simMain: null as WebGLUniformLocation | null,
    simHot: null as WebGLUniformLocation | null,
    blurDir: null as WebGLUniformLocation | null,
    blurExt: null as WebGLUniformLocation | null,
    blurTex: null as WebGLUniformLocation | null,
    blurRes: null as WebGLUniformLocation | null,
    compScene: null as WebGLUniformLocation | null,
    compGlow: null as WebGLUniformLocation | null,
  }

  // 渲染循环读缓存值，避免每帧触发 Vue 依赖追踪（参考实现同款优化）
  let cachedActive = false
  let cachedSlider = 0.7
  let cachedColors: FirePalette = themeFirePalette()

  // 主题色响应式跟随：组件层把最新 palette 写进这个 ref
  const paletteRef: Ref<FirePalette> = ref(cachedColors)
  watch(
    paletteRef,
    (p) => {
      cachedColors = p
    },
    { immediate: true },
  )
  /** 供组件在主题切换时调用 */
  function setPalette(p: FirePalette) {
    paletteRef.value = p
  }

  watch(isActive, (v) => (cachedActive = v), { immediate: true })
  watch(sliderValue, (v) => (cachedSlider = v / 100), { immediate: true })

  watch(
    isActive,
    (now) => {
      console.info('[effort-fire] active→', now)
      if (now && ultraStart == null) ultraStart = performance.now()
      else if (!now) ultraStart = null
      if (now) ensureLoop()
    },
    { flush: 'post' },
  )

  onMounted(() => nextTick(init))

  onBeforeUnmount(() => {
    if (rafId != null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    resizeObserver?.disconnect()
    resizeObserver = null
    if (resizeDebounce) clearTimeout(resizeDebounce)
    loopRunning = false
    destroyFBOs()
    destroyPrograms()
    if (canvasEl) {
      canvasEl.removeEventListener('webglcontextlost', onContextLost)
      canvasEl.removeEventListener('webglcontextrestored', onContextRestored)
    }
    gl = null
    canvasEl = null
  })

  function onContextLost(e: Event) {
    e.preventDefault()
  }
  function onContextRestored() {
    programsReady = false
    compilePrograms()
    if (programsReady) {
      resize()
      if (cachedActive) ensureLoop()
    }
  }

  /**
   * 初始化（幂等）：弹层每次打开都会挂载新 canvas，
   * 通过 watch(canvasRef) 触发；同一 canvas 重复触发直接跳过。
   */
  function init() {
    const canvas = canvasRef.value
    if (!canvas || canvas === canvasEl) return

    // 若绑定在旧 canvas 上，先全量释放
    if (rafId != null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    loopRunning = false
    if (canvasEl) {
      canvasEl.removeEventListener('webglcontextlost', onContextLost)
      canvasEl.removeEventListener('webglcontextrestored', onContextRestored)
    }
    destroyFBOs()
    destroyPrograms()

    const ctx = canvas.getContext('webgl2', {
      preserveDrawingBuffer: false,
      antialias: false,
    })
    if (!ctx) {
      console.error('[effort-fire] WebGL2 unavailable')
      supported.value = false
      canvasEl = null
      gl = null
      return
    }
    supported.value = true
    gl = ctx
    canvasEl = canvas
    canvas.addEventListener('webglcontextlost', onContextLost)
    canvas.addEventListener('webglcontextrestored', onContextRestored)
    compilePrograms()
    if (!programsReady) {
      supported.value = false
      return
    }
    resizeObserver?.disconnect()
    resizeObserver = new ResizeObserver(() => {
      if (resizeDebounce) clearTimeout(resizeDebounce)
      resizeDebounce = setTimeout(resize, 80)
    })
    resizeObserver.observe(canvas)

    console.info('[effort-fire] WebGL2 context ready', {
      w: canvas.getBoundingClientRect().width,
      h: canvas.getBoundingClientRect().height,
      active: cachedActive,
    })

    // 已处于最大档时立即点火（弹层打开即见火焰）
    if (cachedActive) ensureLoop()
  }

  // 弹层开→canvas 挂载→初始化；关→置空。flush post 确保元素已入 DOM。
  watch(canvasRef, (el) => {
    if (el) void nextTick(init)
  })

  /** WebGL 不可用/编译失败时为 false，组件据此显示 CSS 兜底动效 */
  const supported = ref(true)

  function resize() {
    if (!gl || !canvasEl) return
    const rect = canvasEl.getBoundingClientRect()
    if (!rect.width || !rect.height) {
      console.info('[effort-fire] resize skipped: no size', { w: rect.width, h: rect.height })
      return
    }
    const dpr = window.devicePixelRatio
    canvasEl.width = Math.round(rect.width * dpr)
    canvasEl.height = Math.round(rect.height * dpr)
    destroyFBOs()
    createFBOs()
    console.info('[effort-fire] resize', {
      css: `${rect.width}x${rect.height}`,
      px: `${canvasEl.width}x${canvasEl.height}`,
      fbo: !!(simA && simB),
    })
    // 弹层首次布局完成后 FBO 才可用：此时若已处于最大档，补启渲染循环
    if (cachedActive) ensureLoop()
  }

  function compileShader(type: number, src: string): WebGLShader | null {
    if (!gl) return null
    const sh = gl.createShader(type)
    if (!sh) return null
    gl.shaderSource(sh, src)
    gl.compileShader(sh)
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      console.error(gl.getShaderInfoLog(sh))
      gl.deleteShader(sh)
      return null
    }
    return sh
  }

  function linkProgram(vsSrc: string, fsSrc: string): WebGLProgram | null {
    if (!gl) return null
    const v = compileShader(gl.VERTEX_SHADER, vsSrc)
    const f = compileShader(gl.FRAGMENT_SHADER, fsSrc)
    if (!v || !f) return null
    const p = gl.createProgram()
    if (!p) return null
    gl.attachShader(p, v)
    gl.attachShader(p, f)
    gl.bindAttribLocation(p, 0, 'a_pos')
    gl.linkProgram(p)
    gl.deleteShader(v)
    gl.deleteShader(f)
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      console.error(gl.getProgramInfoLog(p))
      return null
    }
    return p
  }

  function compilePrograms() {
    if (!gl) return
    simProg = linkProgram(VERT, FRAG_SIM)
    blurProg = linkProgram(VERT, FRAG_BLUR)
    compProg = linkProgram(VERT, FRAG_COMP)
    if (!simProg || !blurProg || !compProg) return

    vao = gl.createVertexArray()
    gl.bindVertexArray(vao)
    vbo = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo)
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW,
    )
    gl.enableVertexAttribArray(0)
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)

    U.simTime = gl.getUniformLocation(simProg, 'u_time')
    U.simSlider = gl.getUniformLocation(simProg, 'u_slider')
    U.simElapsed = gl.getUniformLocation(simProg, 'u_elapsed')
    U.simBack = gl.getUniformLocation(simProg, 'u_back')
    U.simDeep = gl.getUniformLocation(simProg, 'u_cdeep')
    U.simMain = gl.getUniformLocation(simProg, 'u_cmain')
    U.simHot = gl.getUniformLocation(simProg, 'u_chot')
    U.blurDir = gl.getUniformLocation(blurProg, 'u_dir')
    U.blurExt = gl.getUniformLocation(blurProg, 'u_ext')
    U.blurTex = gl.getUniformLocation(blurProg, 'u_tex')
    U.blurRes = gl.getUniformLocation(blurProg, 'u_res')
    U.compScene = gl.getUniformLocation(compProg, 'u_scene')
    U.compGlow = gl.getUniformLocation(compProg, 'u_glow')

    programsReady = true
  }

  function makeFBO(): FBO | null {
    if (!gl || !canvasEl) return null
    const fbo = gl.createFramebuffer()
    const tex = gl.createTexture()
    if (!fbo || !tex) return null
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo)
    gl.bindTexture(gl.TEXTURE_2D, tex)
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      canvasEl.width,
      canvasEl.height,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      null,
    )
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0)
    gl.clearColor(0, 0, 0, 1)
    gl.clear(gl.COLOR_BUFFER_BIT)
    return { fbo, tex }
  }

  function createFBOs() {
    if (!gl || !canvasEl) return
    simA = makeFBO()
    simB = makeFBO()
    blurH = makeFBO()
    blurV = makeFBO()
    if (!simA || !simB || !blurH || !blurV) destroyFBOs()
  }

  function destroyFBO(entry: FBO | null) {
    if (!gl || !entry) return
    gl.deleteFramebuffer(entry.fbo)
    gl.deleteTexture(entry.tex)
  }

  function destroyFBOs() {
    destroyFBO(simA)
    simA = null
    destroyFBO(simB)
    simB = null
    destroyFBO(blurH)
    blurH = null
    destroyFBO(blurV)
    blurV = null
  }

  function destroyPrograms() {
    if (!gl) return
    if (simProg) gl.deleteProgram(simProg)
    if (blurProg) gl.deleteProgram(blurProg)
    if (compProg) gl.deleteProgram(compProg)
    if (vao) gl.deleteVertexArray(vao)
    if (vbo) gl.deleteBuffer(vbo)
    simProg = blurProg = compProg = null
    vao = null
    vbo = null
    programsReady = false
  }

  function ensureLoop() {
    if (!simA || !simB) {
      resize()
      if (!simA || !simB) {
        console.info('[effort-fire] ensureLoop: no FBO yet', {
          hasCanvas: !!canvasEl,
          size: canvasEl?.getBoundingClientRect(),
        })
        return
      }
    }
    // 减弱动态效果：不跑连续动画，只画一帧已点燃的静态火焰
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      drawFrame(performance.now(), 3.0)
      return
    }
    if (loopRunning) {
      idleFrames = 0
      return
    }
    loopRunning = true
    idleFrames = 0
    wasActive = false
    if (gl && simA && simB) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, simA.fbo)
      gl.clear(gl.COLOR_BUFFER_BIT)
      gl.bindFramebuffer(gl.FRAMEBUFFER, simB.fbo)
      gl.clear(gl.COLOR_BUFFER_BIT)
    }
    console.info('[effort-fire] loop started', { cachedSlider, cachedActive })
    rafId = requestAnimationFrame(render)
  }

  function render(t: number) {
    if (!gl || !canvasEl || !simA || !simB) {
      loopRunning = false
      return
    }
    const active = cachedActive

    if (!active && !wasActive) {
      if (++idleFrames > MAX_IDLE) {
        loopRunning = false
        rafId = null
        return
      }
      rafId = requestAnimationFrame(render)
      return
    }

    idleFrames = 0

    if (active && !wasActive) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, simA.fbo)
      gl.clear(gl.COLOR_BUFFER_BIT)
      gl.bindFramebuffer(gl.FRAMEBUFFER, simB.fbo)
      gl.clear(gl.COLOR_BUFFER_BIT)
    }
    wasActive = active

    const elapsed = active ? (performance.now() - (ultraStart ?? 0)) / 1000 : -1.0
    drawFrame(t, elapsed)

    // ping-pong 交换
    const tmp = simA
    simA = simB
    simB = tmp

    rafId = requestAnimationFrame(render)
  }

  /** 单帧四 pass 绘制（模拟→横模糊→纵模糊→合成） */
  let framesDrawn = 0
  function drawFrame(t: number, elapsed: number) {
    if (!gl || !canvasEl || !simA || !simB || !blurH || !blurV) return
    const sv = cachedSlider
    const col = cachedColors
    if (++framesDrawn <= 3) {
      console.info('[effort-fire] frame', framesDrawn, {
        sv,
        elapsed,
        canvas: `${canvasEl.width}x${canvasEl.height}`,
      })
    }

    gl.viewport(0, 0, canvasEl.width, canvasEl.height)

    // pass 1: 模拟
    gl.bindFramebuffer(gl.FRAMEBUFFER, simB.fbo)
    gl.useProgram(simProg!)
    gl.uniform1f(U.simTime, t * 0.001)
    gl.uniform1f(U.simSlider, sv)
    gl.uniform1f(U.simElapsed, elapsed)
    gl.uniform3f(U.simDeep, col.deep[0], col.deep[1], col.deep[2])
    gl.uniform3f(U.simMain, col.main[0], col.main[1], col.main[2])
    gl.uniform3f(U.simHot, col.hot[0], col.hot[1], col.hot[2])
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, simA.tex)
    gl.uniform1i(U.simBack, 0)
    gl.drawArrays(gl.TRIANGLES, 0, 6)

    // pass 2: 横向模糊
    gl.useProgram(blurProg!)
    gl.uniform2f(U.blurRes, canvasEl.width, canvasEl.height)
    gl.bindFramebuffer(gl.FRAMEBUFFER, blurH.fbo)
    gl.uniform2f(U.blurDir, 1.0, 0.0)
    gl.uniform1f(U.blurExt, 1.0)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, simB.tex)
    gl.uniform1i(U.blurTex, 0)
    gl.drawArrays(gl.TRIANGLES, 0, 6)

    // pass 3: 纵向模糊
    gl.bindFramebuffer(gl.FRAMEBUFFER, blurV.fbo)
    gl.uniform2f(U.blurDir, 0.0, 1.0)
    gl.uniform1f(U.blurExt, 0.0)
    gl.bindTexture(gl.TEXTURE_2D, blurH.tex)
    gl.drawArrays(gl.TRIANGLES, 0, 6)

    // pass 4: 合成到屏幕
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    gl.useProgram(compProg!)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, simB.tex)
    gl.uniform1i(U.compScene, 0)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, blurV.tex)
    gl.uniform1i(U.compGlow, 1)
    gl.drawArrays(gl.TRIANGLES, 0, 6)

    // ping-pong 交换（供下一帧模拟读取）
    const tmp = simA
    simA = simB
    simB = tmp
  }

  return { setPalette, supported }
}
