<template>
  <div class="space-y-3">
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-6 gap-3">
      <Card class="panel-corner">
        <template #title><span class="section-title">Instances</span></template>
        <template #content><div class="mono-data text-2xl led-cyan">{{ rows.length }}</div></template>
      </Card>
      <Card class="panel-corner">
        <template #title><span class="section-title">Running</span></template>
        <template #content><div class="mono-data text-2xl led-cyan">{{ runningCount }}</div></template>
      </Card>
      <Card class="panel-corner">
        <template #title><span class="section-title">CPU</span></template>
        <template #content><div class="mono-data text-2xl led-cyan">{{ metrics ? `${metrics.cpu.usage_percent.toFixed(1)}%` : '--' }}</div></template>
      </Card>
      <Card class="panel-corner">
        <template #title><span class="section-title">RAM</span></template>
        <template #content><div class="mono-data text-2xl led-cyan">{{ metrics ? `${metrics.ram.usage_percent.toFixed(1)}%` : '--' }}</div></template>
      </Card>
      <Card class="panel-corner">
        <template #title><span class="section-title">GPU</span></template>
        <template #content>
          <div class="mono-data text-2xl led-cyan">{{ gpuSummary }}</div>
          <div class="mono-data text-[10px] text-zinc-400 mt-1">{{ metrics?.rocm.available ? 'rocm-smi online' : 'rocm-smi offline' }}</div>
        </template>
      </Card>
      <Card class="panel-corner">
        <template #title><span class="section-title">Actions</span></template>
        <template #content>
          <div class="flex gap-2">
            <Button label="Refresh" icon="pi pi-refresh" class="action-refresh" @click="load" />
            <Button label="Create" icon="pi pi-plus" class="action-create" @click="showCreate = true" />
          </div>
        </template>
      </Card>
    </div>

    <Card class="panel-corner">
      <template #title><span class="section-title">Instance Matrix</span></template>
      <template #content>
        <DataTable
          :value="rows"
          data-key="name"
          size="small"
          selection-mode="single"
          v-model:selection="selected"
          sort-mode="single"
          sort-field="display_name"
          :sort-order="1"
        >
          <Column header="Instance" sortable>
            <template #body="{ data }">
              <span class="mono-data">{{ data.display_name || data.name }}</span>
            </template>
          </Column>
          <Column field="family" header="Family" sortable />
          <Column field="model" header="Model" sortable />
          <Column field="quant" header="Quant" sortable />
          <Column field="variant" header="Variant" sortable />
          <Column field="port" header="Port" sortable />
          <Column field="status" header="Status" sortable>
            <template #body="{ data }">
              <span class="mono-data" :class="statusClass(data.status)">{{ data.status.toUpperCase() }}</span>
            </template>
          </Column>
          <Column header="Mem Fit">
            <template #body="{ data }">
              <span class="mono-data" :class="memoryFitClass(memoryPreviewOf(data.name))">
                {{ memoryFitBadge(memoryPreviewOf(data.name)) }}
              </span>
            </template>
          </Column>
          <Column header="Ops">
            <template #body="{ data }">
              <div class="flex gap-1">
                <Button label="Start" size="small" @click="start(data.name)" />
                <Button label="Stop" size="small" severity="warn" @click="stop(data.name)" />
                <Button label="Restart" size="small" class="action-restart" @click="restart(data.name)" />
                <Button label="Edit" size="small" class="action-edit" @click="openEdit(data.name)" />
                <Button label="Delete" size="small" severity="danger" @click="confirmDelete(data.name)" />
              </div>
            </template>
          </Column>
        </DataTable>
      </template>
    </Card>

    <div v-if="selected" class="grid grid-cols-1 xl:grid-cols-12 gap-3">
      <Card class="panel-corner xl:col-span-7">
        <template #title><span class="section-title">Selected Parameters</span></template>
        <template #content>
          <div class="grid grid-cols-1 md:grid-cols-2 gap-2 mono-data text-[11px]">
            <div class="param-block">
              <div class="param-title">Runtime</div>
              <div class="param-row"><span>Instance</span><span>{{ selectedDisplayName }}</span></div>
              <div class="param-row"><span>Family</span><span>{{ selected.family || '-' }}</span></div>
              <div class="param-row"><span>Model</span><span>{{ selected.model || '-' }}</span></div>
              <div class="param-row"><span>Quant</span><span>{{ selected.quant || '-' }}</span></div>
              <div class="param-row"><span>Variant</span><span>{{ selected.variant || '-' }}</span></div>
              <div class="param-row"><span>Model Ref</span><span>{{ selected.config.model || '-' }}</span></div>
              <div class="param-row"><span>MMProj</span><span>{{ selected.config.mmproj || '-' }}</span></div>
              <div class="param-row"><span>Port</span><span>{{ selected.config.host_port || '-' }} -> {{ selected.config.port || '-' }}</span></div>
              <div class="param-row"><span>Status</span><span :class="statusClass(selected.status)">{{ selected.status.toUpperCase() }}</span></div>
            </div>
            <div class="param-block">
              <div class="param-title">Execution</div>
              <div class="param-row"><span>Ctx Size</span><span>{{ selected.config.ctx_size ?? '-' }}</span></div>
              <div class="param-row"><span>Threads</span><span>{{ selected.config.threads ?? '-' }}</span></div>
              <div class="param-row"><span>GPU Layers</span><span>{{ selected.config.n_gpu_layers ?? selected.config.gpu_layers ?? '-' }}</span></div>
              <div class="param-row"><span>Parallel</span><span>{{ selected.config.parallel ?? '-' }}</span></div>
              <div class="param-row"><span>Thinking</span><span>{{ selected.config.thinking ?? '-' }}</span></div>
            </div>
            <div class="param-block">
              <div class="param-title">Sampling</div>
              <div class="param-row"><span>Temp</span><span>{{ selected.config.sampling?.temp ?? '-' }}</span></div>
              <div class="param-row"><span>Top-P</span><span>{{ selected.config.sampling?.top_p ?? '-' }}</span></div>
              <div class="param-row"><span>Top-K</span><span>{{ selected.config.sampling?.top_k ?? '-' }}</span></div>
              <div class="param-row"><span>Min-P</span><span>{{ selected.config.sampling?.min_p ?? '-' }}</span></div>
            </div>
            <div class="param-block">
              <div class="param-title">Cache</div>
              <div class="param-row"><span>K</span><span>{{ selected.config.cache_type_k ?? '-' }}</span></div>
              <div class="param-row"><span>V</span><span>{{ selected.config.cache_type_v ?? '-' }}</span></div>
            </div>
            <div class="param-block">
              <div class="param-title">Memory Preview</div>
              <div class="param-row"><span>Weights</span><span>{{ formatBytes(selectedPreview?.model_bytes) }}</span></div>
              <div class="param-row"><span>KV Cache</span><span>{{ formatBytes(selectedPreview?.kv_cache_bytes) }}</span></div>
              <div class="param-row"><span>Overhead</span><span>{{ formatBytes(selectedPreview?.overhead_bytes) }}</span></div>
              <div class="param-row"><span>Load Est</span><span>{{ formatBytes(selectedPreview?.estimated_total_bytes) }}</span></div>
              <div class="param-row"><span>Avail RAM</span><span>{{ metrics ? formatBytes(Number(metrics.ram.available_mb) * 1024 * 1024) : '--' }}</span></div>
              <div class="param-row"><span>Fit</span><span :class="memoryFitClass(selectedPreview)">{{ memoryFitLabel(selectedPreview) }}</span></div>
              <div v-if="selectedPreview?.warning" class="mono-data text-[10px] text-amber-400 mt-1">{{ selectedPreview.warning }}</div>
            </div>
          </div>
        </template>
      </Card>

      <Card class="panel-corner xl:col-span-5">
        <template #title>
          <span class="section-title">
            Logs
            <span v-if="selected" class="mono-data text-[10px] text-zinc-400">[{{ selectedDisplayName }}]</span>
          </span>
        </template>
        <template #content>
          <div class="flex items-center gap-2 mb-2">
            <Select v-model="logTail" :options="tailOptions" class="w-24" />
            <Button label="Load Logs" icon="pi pi-align-left" @click="loadLogs" />
          </div>
          <div class="mono-data text-[11px] text-zinc-300 whitespace-pre-wrap max-h-72 overflow-auto border border-zinc-800 bg-zinc-950 p-2">
            {{ logsText }}
          </div>
        </template>
      </Card>
    </div>

    <Card class="panel-corner" v-if="metrics">
      <template #title><span class="section-title">GPU Telemetry</span></template>
      <template #content>
        <DataTable :value="metrics.rocm.devices" size="small" data-key="id">
          <Column field="id" header="GPU" />
          <Column field="name" header="Name" />
          <Column header="Util">
            <template #body="{ data }">
              <span class="mono-data">{{ data.utilization_percent != null ? `${Number(data.utilization_percent).toFixed(1)}%` : '-' }}</span>
            </template>
          </Column>
          <Column header="VRAM">
            <template #body="{ data }">
              <span class="mono-data">{{ data.memory_use_percent != null ? `${Number(data.memory_use_percent).toFixed(1)}%` : '-' }}</span>
            </template>
          </Column>
          <Column header="Temp">
            <template #body="{ data }">
              <span class="mono-data">{{ data.temperature_c != null ? `${Number(data.temperature_c).toFixed(1)}°C` : '-' }}</span>
            </template>
          </Column>
        </DataTable>
        <div v-if="!metrics.rocm.available" class="mono-data text-[11px] mt-2 text-amber-500">
          {{ metrics.rocm.error || 'rocm-smi unavailable' }}
        </div>
      </template>
    </Card>

    <Dialog v-model:visible="showCreate" modal header="Create Instance" :style="{ width: '38rem' }">
      <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
        <InputText v-model="createForm.name" placeholder="name" />
        <InputNumber v-model="createForm.port" :use-grouping="false" placeholder="port (optional)" />
        <InputText v-model="createForm.model" placeholder="model path relative to /models" class="md:col-span-2" />
        <InputText v-model="createForm.mmproj" placeholder="mmproj relative path (optional)" class="md:col-span-2" />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showCreate = false" />
        <Button label="Create" @click="create" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showEdit" modal header="Edit Instance Parameter" :style="{ width: '36rem' }">
      <div class="grid grid-cols-1 gap-2">
        <InputText :model-value="editForm.displayName || editForm.name" disabled />
        <div class="mono-data text-[10px] text-zinc-500">id: {{ editForm.name }}</div>
        <InputText v-model="editForm.key" placeholder="key, e.g. llama.sampling.temp / compose.host_port / meta.name" />
        <InputText v-model="editForm.value" placeholder="value (JSON or text)" />
        <div class="mono-data text-[10px] text-zinc-400">prefix sources: llama.* / compose.* / meta.* (raw key path also supported)</div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showEdit = false" />
        <Button label="Apply" @click="applyEdit" />
      </template>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useConfirm } from 'primevue/useconfirm'
import { useToast } from 'primevue/usetoast'
import Button from 'primevue/button'
import Card from 'primevue/card'
import Column from 'primevue/column'
import DataTable from 'primevue/datatable'
import Dialog from 'primevue/dialog'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'

import {
  createInstance,
  deleteInstance,
  editInstance,
  getInstanceMemoryPreviews,
  getInstanceLogs,
  getSystemMetrics,
  listInstancesHierarchy,
  listStatuses,
  restartInstance,
  startInstance,
  stopInstance,
  type InstanceHierarchyInfo,
  type InstanceMemoryPreview,
  type SystemMetrics,
} from '@/api'

interface Row extends InstanceHierarchyInfo {
  model: string
  family: string
  quant: string
  variant: string
  port: number
  status: string
}

const toast = useToast()
const confirm = useConfirm()

const rows = ref<Row[]>([])
const selected = ref<Row | null>(null)
const metrics = ref<SystemMetrics | null>(null)
const memoryPreviews = ref<Record<string, InstanceMemoryPreview>>({})
const logs = ref<string[]>([])
const logTail = ref(200)
const tailOptions = [100, 200, 400, 800]
let liveTimer: number | undefined

const showCreate = ref(false)
const showEdit = ref(false)

const createForm = ref({
  name: '',
  model: '',
  mmproj: '',
  port: null as number | null,
})

const editForm = ref({
  name: '',
  displayName: '',
  key: '',
  value: '',
})

const runningCount = computed(() => rows.value.filter((row) => normalizeStatus(row.status) === 'running').length)
const gpuSummary = computed(() => {
  const devices = metrics.value?.rocm.devices || []
  if (!devices.length) return '--'
  const values = devices.map((item) => item.utilization_percent).filter((item): item is number => typeof item === 'number')
  if (!values.length) return `${devices.length} card`
  const avg = values.reduce((sum, item) => sum + item, 0) / values.length
  return `${avg.toFixed(1)}%`
})
const logsText = computed(() => (logs.value.length ? logs.value.join('\n') : '<empty>'))
const selectedPreview = computed(() => (selected.value ? memoryPreviews.value[selected.value.name] ?? null : null))
const selectedDisplayName = computed(() => selected.value?.display_name || selected.value?.name || '-')

onMounted(async () => {
  await load()
  liveTimer = window.setInterval(() => {
    void refreshLive()
  }, 3000)
})

onUnmounted(() => {
  if (liveTimer) window.clearInterval(liveTimer)
})

watch(selected, async () => {
  await loadLogs()
})
watch(logTail, async () => {
  await loadLogs()
})

async function load() {
  const [instances, statuses, system, previews] = await Promise.all([
    listInstancesHierarchy(),
    listStatuses(),
    getSystemMetrics().catch(() => null),
    getInstanceMemoryPreviews().catch(() => []),
  ])
  const statusMap = new Map<string, string>(statuses.map((item) => [item.name, item.status || 'unknown']))
  memoryPreviews.value = Object.fromEntries(previews.map((item) => [item.name, item]))
  rows.value = instances.map((instance) => ({
    ...instance,
    display_name: String(instance.display_name || instance.name || ''),
    model: String(instance.model || ''),
    family: String(instance.family || ''),
    quant: String(instance.quant || ''),
    variant: String(instance.variant || 'general'),
    port: Number(instance.host_port || instance.config.host_port || 0),
    status: statusMap.get(instance.name) ?? 'unknown',
  }))
  metrics.value = system
  if (selected.value) {
    selected.value = rows.value.find((row) => row.name === selected.value?.name) || null
  }
  if (!selected.value && rows.value.length > 0) {
    selected.value = rows.value[0]
  }
}

async function refreshLive() {
  const [statuses, system] = await Promise.all([listStatuses().catch(() => []), getSystemMetrics().catch(() => null)])
  const statusMap = new Map<string, string>(statuses.map((item) => [item.name, item.status || 'unknown']))
  rows.value = rows.value.map((row) => ({
    ...row,
    status: statusMap.get(row.name) ?? row.status,
  }))
  metrics.value = system
  if (selected.value) {
    selected.value = rows.value.find((row) => row.name === selected.value?.name) || selected.value
  }
}

async function loadLogs() {
  if (!selected.value) {
    logs.value = []
    return
  }
  const text = await getInstanceLogs(selected.value.name, logTail.value)
  logs.value = text.split('\n')
}

async function create() {
  await createInstance({
    name: createForm.value.name,
    model: createForm.value.model,
    mmproj: createForm.value.mmproj || null,
    port: createForm.value.port ?? null,
  })
  toast.add({ severity: 'success', summary: 'Created', detail: createForm.value.name, life: 2000 })
  showCreate.value = false
  createForm.value = { name: '', model: '', mmproj: '', port: null }
  await load()
}

function openEdit(name: string) {
  editForm.value = { name, displayName: displayNameByName(name), key: '', value: '' }
  showEdit.value = true
}

async function applyEdit() {
  await editInstance(editForm.value.name, editForm.value.key, parseValue(editForm.value.value))
  toast.add({
    severity: 'success',
    summary: 'Updated',
    detail: `${editForm.value.displayName || editForm.value.name}.${editForm.value.key}`,
    life: 2200,
  })
  showEdit.value = false
  await load()
}

async function start(name: string) {
  await startInstance(name)
  toast.add({ severity: 'success', summary: 'Started', detail: displayNameByName(name), life: 1600 })
  await load()
}

async function stop(name: string) {
  await stopInstance(name)
  toast.add({ severity: 'warn', summary: 'Stopped', detail: displayNameByName(name), life: 1600 })
  await load()
}

async function restart(name: string) {
  await restartInstance(name)
  toast.add({ severity: 'info', summary: 'Restarted', detail: displayNameByName(name), life: 1600 })
  await load()
}

function confirmDelete(name: string) {
  const displayName = displayNameByName(name)
  confirm.require({
    message: `Delete instance ${displayName}?`,
    acceptClass: 'p-button-danger',
    accept: async () => {
      await deleteInstance(name)
      toast.add({ severity: 'success', summary: 'Deleted', detail: displayName, life: 1600 })
      await load()
    },
  })
}

function displayNameByName(name: string) {
  return rows.value.find((row) => row.name === name)?.display_name || name
}

function parseValue(input: string) {
  try {
    return JSON.parse(input)
  } catch {
    return input
  }
}

function normalizeStatus(status: string) {
  const text = status.toLowerCase()
  if (text.includes('up') || text.includes('run')) return 'running'
  if (text.includes('stop') || text.includes('exit')) return 'stopped'
  return 'unknown'
}

function statusClass(status: string) {
  const normalized = normalizeStatus(status)
  if (normalized === 'running') return 'status-running'
  if (normalized === 'stopped') return 'status-stopped'
  return 'status-unknown'
}

function memoryPreviewOf(name: string) {
  return memoryPreviews.value[name]
}

function memoryFitState(preview?: InstanceMemoryPreview | null) {
  if (!preview || !preview.estimated_total_bytes || !metrics.value) return 'unknown'
  const available = Number(metrics.value.ram.available_mb) * 1024 * 1024
  if (!available) return 'unknown'
  const ratio = Number(preview.estimated_total_bytes) / available
  if (ratio <= 0.75) return 'good'
  if (ratio <= 1.0) return 'warn'
  return 'bad'
}

function memoryFitClass(preview?: InstanceMemoryPreview | null) {
  const state = memoryFitState(preview)
  if (state === 'good') return 'mem-fit-good'
  if (state === 'warn') return 'mem-fit-warn'
  if (state === 'bad') return 'mem-fit-bad'
  return 'text-zinc-500'
}

function memoryFitLabel(preview?: InstanceMemoryPreview | null) {
  const state = memoryFitState(preview)
  if (state === 'good') return 'GOOD'
  if (state === 'warn') return 'WARN'
  if (state === 'bad') return 'LOW'
  return 'N/A'
}

function memoryFitBadge(preview?: InstanceMemoryPreview | null) {
  if (!preview) return 'N/A'
  return `${memoryFitLabel(preview)} ${formatBytes(preview.estimated_total_bytes, true)}`
}

function formatBytes(bytes?: number, compact = false) {
  if (bytes == null || Number.isNaN(Number(bytes)) || Number(bytes) <= 0) return '--'
  const units = compact ? ['B', 'K', 'M', 'G', 'T', 'P'] : ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  let value = Number(bytes)
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value.toFixed(1)}${compact ? '' : ' '}${units[unit]}`
}
</script>
