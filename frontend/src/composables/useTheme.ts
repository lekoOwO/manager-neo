import { ref, watch } from 'vue'

type PresetName = 'aura' | 'lara' | 'nora'
type MenuMode = 'static' | 'overlay'

const storage = {
  preset: 'manager-neo.theme.preset',
  menuMode: 'manager-neo.theme.menuMode',
}

const darkMode = ref(true)
const preset = ref<PresetName>((localStorage.getItem(storage.preset) as PresetName) || 'aura')
const menuMode = ref<MenuMode>((localStorage.getItem(storage.menuMode) as MenuMode) || 'static')

const cyanPalette = {
  50: '{cyan.50}',
  100: '{cyan.100}',
  200: '{cyan.200}',
  300: '{cyan.300}',
  400: '{cyan.400}',
  500: '{cyan.500}',
  600: '{cyan.600}',
  700: '{cyan.700}',
  800: '{cyan.800}',
  900: '{cyan.900}',
  950: '{cyan.950}',
}

const zincSurface = {
  0: '#18181b',
  50: '{zinc.50}',
  100: '{zinc.100}',
  200: '{zinc.200}',
  300: '{zinc.300}',
  400: '{zinc.400}',
  500: '{zinc.500}',
  600: '{zinc.600}',
  700: '{zinc.700}',
  800: '{zinc.800}',
  900: '{zinc.900}',
  950: '{zinc.950}',
}

async function applyTheme() {
  const { updatePreset, usePreset } = await import('@primeuix/themes')
  const presetModule =
    preset.value === 'lara'
      ? await import('@primeuix/themes/lara')
      : preset.value === 'nora'
      ? await import('@primeuix/themes/nora')
      : await import('@primeuix/themes/aura')

  await usePreset(presetModule.default)
  updatePreset({
    semantic: {
      primary: cyanPalette,
      colorScheme: {
        light: { surface: zincSurface },
        dark: { surface: zincSurface },
      },
    },
  })

  document.documentElement.classList.add('app-dark')
}

watch(
  [preset, menuMode],
  () => {
    localStorage.setItem(storage.preset, preset.value)
    localStorage.setItem(storage.menuMode, menuMode.value)
    applyTheme()
  },
  { immediate: true },
)

export function useTheme() {
  return {
    darkMode,
    preset,
    menuMode,
    applyTheme,
  }
}
