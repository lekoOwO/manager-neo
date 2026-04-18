<template>
  <header class="h-14 border-b border-zinc-800 bg-zinc-950 px-3 lg:px-5 flex items-center justify-between panel-corner">
    <div class="flex items-center gap-3">
      <Button
        icon="pi pi-bars"
        class="lg:hidden"
        outlined
        severity="secondary"
        @click="$emit('toggleMenu')"
      />
      <div>
        <h1 class="text-xs uppercase tracking-[0.18em] font-semibold text-zinc-100">
          Manager Neo Control Grid
        </h1>
        <div class="mono-data text-[10px] text-zinc-500 flex items-center gap-2">
          <span class="led-cyan">●</span>
          LOCAL NODE OPERATIONAL
        </div>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <span class="mono-data text-[11px] text-zinc-400 hidden lg:inline">{{ now }}</span>
      <Button icon="pi pi-sliders-h" severity="secondary" @click="toggleSettings" />
      <Popover ref="settingsPopover">
        <div class="space-y-3 w-56">
          <div class="section-title">Theme Settings</div>
          <div class="space-y-1">
            <div class="text-[10px] uppercase tracking-[0.15em] text-zinc-400">Preset</div>
            <Select
              v-model="preset"
              :options="presetOptions"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <div class="space-y-1">
            <div class="text-[10px] uppercase tracking-[0.15em] text-zinc-400">Menu Mode</div>
            <Select
              v-model="menuMode"
              :options="menuModes"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
        </div>
      </Popover>
    </div>
  </header>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import Button from 'primevue/button'
import Popover from 'primevue/popover'
import Select from 'primevue/select'
import { useTheme } from '@/composables/useTheme'

const { preset, menuMode } = useTheme()

defineEmits<{
  toggleMenu: []
}>()

const now = ref('')
const settingsPopover = ref()
let timer: number | undefined

const menuModes = [
  { label: 'Static', value: 'static' },
  { label: 'Overlay', value: 'overlay' },
]

const presetOptions = [
  { label: 'Aura', value: 'aura' },
  { label: 'Lara', value: 'lara' },
  { label: 'Nora', value: 'nora' },
]

onMounted(() => {
  const update = () => {
    now.value = new Date().toISOString().replace('T', ' ').slice(0, 19)
  }
  update()
  timer = window.setInterval(update, 1000)
})

onUnmounted(() => {
  if (timer) window.clearInterval(timer)
})

function toggleSettings(event: Event) {
  settingsPopover.value?.toggle(event)
}
</script>
