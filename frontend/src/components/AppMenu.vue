<template>
  <nav class="w-full h-full flex flex-col border-r border-zinc-800 bg-zinc-950 panel-corner">
    <div class="px-3 py-3 border-b border-zinc-800">
      <div class="section-title">Modules</div>
    </div>
    <div class="flex-1 p-2 space-y-1">
      <button
        v-for="item in items"
        :key="item.to"
        class="w-full h-10 border rounded-none text-left px-3 flex items-center justify-between transition-none"
        :class="
          isActive(item.to)
            ? 'border-cyan-400 bg-cyan-400 text-black'
            : 'border-zinc-800 bg-zinc-900 text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100'
        "
        @click="$emit('navigate', item.to)"
      >
        <span class="flex items-center gap-2">
          <i :class="[item.icon, 'text-xs']" />
          <span class="text-[11px] uppercase tracking-[0.15em] font-semibold">{{ item.label }}</span>
        </span>
        <span class="mono-data text-[10px]">{{ isActive(item.to) ? 'ON' : 'STBY' }}</span>
      </button>
    </div>
  </nav>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'

defineProps<{
  items: { label: string; icon: string; to: string }[]
}>()

defineEmits<{
  navigate: [path: string]
}>()

const route = useRoute()
const isActive = (path: string) => route.path === path
</script>
