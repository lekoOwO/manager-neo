import { createApp } from 'vue'
import PrimeVue from 'primevue/config'
import Aura from '@primeuix/themes/aura'
import ToastService from 'primevue/toastservice'
import ConfirmationService from 'primevue/confirmationservice'

import App from './App.vue'
import router from './router'
import './style.css'
import './assets/prime-overrides.css'

const industrialPt = {
  button: {
    root: 'rounded-none border border-zinc-700 bg-zinc-900 text-zinc-100 shadow-none transition-none hover:bg-cyan-400 hover:text-black hover:border-cyan-400 active:bg-cyan-500 focus:outline-none focus:ring-0 disabled:opacity-45',
    label: 'uppercase tracking-[0.15em] text-[11px] font-semibold',
  },
  card: {
    root: 'industrial-panel rounded-none border border-zinc-800 bg-zinc-900 shadow-none',
    header: 'hidden',
    body: 'p-4',
    title: 'uppercase tracking-[0.15em] text-xs text-zinc-200',
    content: 'text-zinc-300 text-xs',
  },
  panel: {
    root: 'industrial-panel rounded-none border border-zinc-800 bg-zinc-900 shadow-none',
    header: 'uppercase tracking-[0.15em] text-xs border-b border-zinc-800 bg-zinc-950 text-zinc-200 px-3 py-2',
    content: 'p-3 text-zinc-300 text-xs',
  },
  progressbar: {
    root: 'h-2 rounded-none border border-zinc-800 bg-zinc-950',
    value: 'industrial-progress-value',
    label: 'hidden',
  },
  tabmenu: {
    root: 'border border-zinc-800 rounded-none bg-zinc-900',
    menu: 'flex gap-0',
    item: 'border-r border-zinc-800 last:border-r-0',
    itemLink:
      'rounded-none border-none px-3 py-2 text-[11px] uppercase tracking-[0.15em] font-semibold text-zinc-400 hover:text-black hover:bg-cyan-400 transition-none',
  },
  datatable: {
    root: 'rounded-none border border-zinc-800 bg-zinc-900',
    table: 'font-mono text-xs',
    header: 'bg-zinc-950 border-b border-zinc-800 text-zinc-300 uppercase tracking-[0.14em]',
    headerCell: 'bg-zinc-950 text-zinc-300',
    bodyCell: 'bg-zinc-900 text-zinc-200',
    thead: 'bg-zinc-950',
    tbody: 'bg-zinc-900',
    row: 'border-b border-zinc-800',
    bodyRow: 'hover:bg-zinc-950',
  },
  dialog: {
    root: 'rounded-none border border-zinc-800 bg-zinc-900 shadow-none',
    header: 'rounded-none border-b border-zinc-800 bg-zinc-950 uppercase tracking-[0.15em] text-xs',
    content: 'bg-zinc-900 text-zinc-200',
    footer: 'border-t border-zinc-800 bg-zinc-950',
  },
  inputtext: {
    root: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100 font-mono text-xs focus:border-cyan-400 focus:ring-0',
  },
  inputnumber: {
    input: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100 font-mono text-xs focus:border-cyan-400 focus:ring-0',
  },
  textarea: {
    root: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100 font-mono text-xs focus:border-cyan-400 focus:ring-0',
  },
  autocomplete: {
    root: 'w-full',
    input: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100 font-mono text-xs focus:border-cyan-400 focus:ring-0',
    panel: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100',
    option: 'font-mono text-xs rounded-none text-zinc-100 hover:bg-cyan-400 hover:text-black',
  },
  select: {
    root: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100 text-xs focus:border-cyan-400 focus:ring-0',
    label: 'font-mono text-xs text-zinc-100',
    dropdown: 'text-zinc-400',
    overlay: 'rounded-none border border-zinc-700 bg-zinc-950 text-zinc-100',
    list: 'bg-zinc-950 text-zinc-100',
    option: 'font-mono text-xs rounded-none text-zinc-100 hover:bg-cyan-400 hover:text-black',
    optionLabel: 'text-zinc-100',
  },
  confirmdialog: {
    root: 'rounded-none border border-zinc-800 bg-zinc-900',
  },
  popover: {
    root: 'rounded-none border border-zinc-800 bg-zinc-900 text-zinc-200 shadow-none',
    content: 'bg-zinc-900 text-zinc-200 p-3',
  },
  toast: {
    root: 'font-mono text-xs',
  },
} as const

const app = createApp(App)
app.use(PrimeVue, {
  ripple: false,
  theme: {
    preset: Aura,
    options: {
      prefix: 'p',
      darkModeSelector: '.app-dark',
      cssLayer: false,
    },
  },
  pt: industrialPt,
})
app.use(router)
app.use(ToastService)
app.use(ConfirmationService)
app.mount('#app')
