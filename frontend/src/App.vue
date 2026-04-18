<template>
  <Toast />
  <ConfirmDialog />

  <div class="min-h-screen flex bg-zinc-950 text-zinc-100">
    <aside
      v-if="showDesktopSidebar"
      class="hidden lg:block w-64 shrink-0 border-r border-zinc-800"
    >
      <AppMenu :items="items" @navigate="router.push" />
    </aside>

    <div class="flex-1 min-h-screen flex flex-col">
      <AppHeader @toggle-menu="toggleMenu" />

      <div
        v-if="mobileMenuOpen || overlayDesktopOpen"
        class="fixed inset-0 z-40 bg-black/70"
        @click="closeOverlayMenu"
      >
        <aside class="w-64 h-full bg-zinc-950 border-r border-zinc-800" @click.stop>
          <AppMenu :items="items" @navigate="navigateFromOverlay" />
        </aside>
      </div>

      <main class="flex-1 overflow-auto industrial-grid p-3 lg:p-4">
        <router-view />
      </main>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import ConfirmDialog from 'primevue/confirmdialog'
import Toast from 'primevue/toast'
import AppHeader from '@/components/AppHeader.vue'
import AppMenu from '@/components/AppMenu.vue'
import { useTheme } from '@/composables/useTheme'

const router = useRouter()
const { menuMode } = useTheme()
const mobileMenuOpen = ref(false)
const overlayDesktopOpen = ref(false)

const items = [
  { label: 'Instances', icon: 'pi pi-server', to: '/' },
  { label: 'Families', icon: 'pi pi-sitemap', to: '/templates' },
  { label: 'Models', icon: 'pi pi-database', to: '/models' },
]

const showDesktopSidebar = computed(() => menuMode.value === 'static')

watch(menuMode, (mode) => {
  overlayDesktopOpen.value = mode === 'overlay'
})

function closeOverlayMenu() {
  mobileMenuOpen.value = false
  overlayDesktopOpen.value = false
}

function toggleMenu() {
  if (window.innerWidth < 1024) {
    mobileMenuOpen.value = !mobileMenuOpen.value
    return
  }
  if (menuMode.value === 'overlay') {
    overlayDesktopOpen.value = !overlayDesktopOpen.value
  }
}

function navigateFromOverlay(path: string) {
  closeOverlayMenu()
  router.push(path)
}
</script>
