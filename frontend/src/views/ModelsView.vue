<template>
  <div class="space-y-3">
    <Card class="panel-corner">
      <template #title><span class="section-title">Model Download Console</span></template>
      <template #content>
        <div class="grid grid-cols-1 lg:grid-cols-4 gap-2">
          <InputText v-model="downloadForm.repoId" placeholder="repo_id (e.g. unsloth/Qwen3.5-27B-GGUF)" class="lg:col-span-2 mono-data" />
          <InputText v-model="downloadForm.patterns" placeholder="patterns csv (optional)" class="mono-data" />
          <InputText v-model="downloadForm.localDir" placeholder="local dir (optional)" class="mono-data" />
        </div>
        <div class="mt-2 flex gap-2">
          <Button label="Download" icon="pi pi-download" @click="download" />
          <Button label="Refresh" icon="pi pi-refresh" severity="secondary" @click="load" />
          <Button label="Refresh Tasks" icon="pi pi-history" severity="secondary" @click="loadDownloadTasks" />
        </div>
        <div class="mt-3 border border-zinc-800 p-2 bg-zinc-950 space-y-2" v-if="activeDownload">
          <div class="mono-data text-[11px] text-zinc-300">
            <div>task: {{ activeDownload.id }} | phase: {{ activeDownload.phase.toUpperCase() }}</div>
            <div>target: /models/{{ activeDownload.plan.target_relative_dir }}</div>
            <div>quant: {{ activeDownload.plan.quant }} | model: {{ activeDownload.plan.model }}</div>
          </div>
          <ProgressBar :value="Math.max(0, Math.min(100, Number(activeDownload.progress_percent || 0)))" :show-value="false" />
          <div class="mono-data text-[11px] text-zinc-400 break-all">{{ activeDownload.latest_message }}</div>
          <div v-if="activeDownload.error" class="mono-data text-[11px] text-red-400 break-all">{{ activeDownload.error }}</div>
          <div v-if="activeDownload.output_path" class="mono-data text-[11px] text-cyan-300 break-all">
            output: {{ activeDownload.output_path }}
          </div>
        </div>
        <DataTable v-if="downloadTasks.length" :value="downloadTasks.slice(0, 8)" size="small" class="mt-3">
          <Column field="id" header="Task" />
          <Column field="phase" header="Phase" />
          <Column header="Progress">
            <template #body="{ data }">
              <span class="mono-data">{{ Number(data.progress_percent || 0).toFixed(1) }}%</span>
            </template>
          </Column>
          <Column header="Target">
            <template #body="{ data }">
              <span class="mono-data block break-all" :title="data.plan?.target_relative_dir">{{ data.plan?.target_relative_dir }}</span>
            </template>
          </Column>
        </DataTable>
      </template>
    </Card>

    <div class="space-y-3">
      <Card class="panel-corner">
        <template #title><span class="section-title">Model Directories</span></template>
        <template #content>
          <DataTable
            :value="rows"
            data-key="key"
            selection-mode="single"
            v-model:selection="selected"
            size="small"
          >
            <Column header="Family">
              <template #body="{ data }">
                <span class="mono-data block break-all" :title="data.family">{{ data.family }}</span>
              </template>
            </Column>
            <Column header="Model">
              <template #body="{ data }">
                <span class="mono-data block break-all" :title="data.model">{{ data.model }}</span>
              </template>
            </Column>
            <Column header="Quant">
              <template #body="{ data }">
                <span class="mono-data">{{ data.quant }}</span>
              </template>
            </Column>
            <Column field="filesCount" header="Files" />
            <Column header="Size">
              <template #body="{ data }">
                <span class="mono-data size-nowrap">{{ data.sizeText }}</span>
              </template>
            </Column>
            <Column header="Ops">
              <template #body="{ data }">
                <div class="flex gap-1">
                  <Button icon="pi pi-sitemap" size="small" severity="info" @click="openCreateFamily(data.key)" />
                  <Button icon="pi pi-plus-circle" size="small" severity="success" @click="openCreateInstance(data.key)" />
                  <Button icon="pi pi-pencil" size="small" severity="secondary" @click="openRename(data.key)" />
                  <Button icon="pi pi-trash" size="small" severity="danger" @click="confirmDelete(data.key)" />
                </div>
              </template>
            </Column>
          </DataTable>
        </template>
      </Card>

      <Card class="panel-corner">
        <template #title><span class="section-title">Model Files</span></template>
        <template #content>
          <div v-if="selected" class="mono-data text-[11px] text-zinc-400 mb-2">
            {{ selected.key }}
          </div>
          <DataTable :value="selected?.files || []" size="small">
            <Column header="File">
              <template #body="{ data }">
                <span class="mono-data block break-all" :title="data.name">{{ data.name }}</span>
              </template>
            </Column>
            <Column header="Size">
              <template #body="{ data }"><span class="mono-data size-nowrap">{{ humanSize(data.size_bytes) }}</span></template>
            </Column>
            <Column header="Path">
              <template #body="{ data }">
                <span class="mono-data block break-all" :title="data.path">{{ data.path }}</span>
              </template>
            </Column>
          </DataTable>
        </template>
      </Card>
    </div>

    <Dialog v-model:visible="showRename" modal header="Rename Model Directory" :style="{ width: '32rem' }">
      <div class="space-y-2">
        <InputText v-model="renameForm.current" disabled />
        <InputText v-model="renameForm.next" placeholder="new model directory name" class="mono-data" />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showRename = false" />
        <Button label="Rename" @click="rename" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showCreateFamily" modal header="Create Family from Model" :style="{ width: '34rem' }">
      <div class="space-y-2">
        <InputText v-model="createFamilyForm.modelName" disabled class="mono-data" />
        <InputText v-model="createFamilyForm.name" placeholder="template name" class="mono-data" />
        <InputText v-model="createFamilyForm.family" placeholder="family name" class="mono-data" />
        <InputText v-model="createFamilyForm.description" placeholder="description (optional)" />
        <div class="text-xs mono-data text-zinc-400">
          model: {{ createFamilyForm.modelRef || '-' }}
        </div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showCreateFamily = false" />
        <Button label="Create Family" severity="info" @click="createFamilyFromModel" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showCreateInstance" modal header="Create Instance from Model" :style="{ width: '34rem' }">
      <div class="space-y-2">
        <InputText v-model="createInstanceForm.modelName" disabled class="mono-data" />
        <InputText v-model="createInstanceForm.name" placeholder="instance name" class="mono-data" />
        <InputText v-model="createInstanceForm.port" placeholder="port (optional)" class="mono-data" />
        <div class="text-xs mono-data text-zinc-400">
          model: {{ createInstanceForm.modelRef || '-' }}
        </div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showCreateInstance = false" />
        <Button label="Create Instance" severity="success" @click="createInstanceFromModel" />
      </template>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useConfirm } from 'primevue/useconfirm'
import { useToast } from 'primevue/usetoast'
import Button from 'primevue/button'
import Card from 'primevue/card'
import Column from 'primevue/column'
import DataTable from 'primevue/datatable'
import Dialog from 'primevue/dialog'
import InputText from 'primevue/inputtext'
import ProgressBar from 'primevue/progressbar'

import {
  createInstance,
  createTemplateFromModel,
  deleteModel,
  listModelDownloadTasks,
  listModelsHierarchy,
  renameModel,
  startModelDownloadTask,
  type ModelFileInfo,
  type ModelDownloadTaskStatus,
  type ModelHierarchyInfo,
} from '@/api'

const toast = useToast()
const confirm = useConfirm()

const rows = ref<(ModelHierarchyInfo & { filesCount: number; sizeText: string })[]>([])
const selected = ref<(ModelHierarchyInfo & { filesCount: number; sizeText: string }) | null>(null)
const downloadTasks = ref<ModelDownloadTaskStatus[]>([])
const activeDownload = computed(() => downloadTasks.value.find((task) => task.running) || downloadTasks.value[0] || null)
let downloadPollTimer: number | undefined
let hadRunningDownload = false

const showRename = ref(false)
const renameForm = ref({ current: '', next: '' })
const showCreateFamily = ref(false)
const showCreateInstance = ref(false)
const createFamilyForm = ref({
  modelName: '',
  name: '',
  family: '',
  description: '',
  modelRef: '',
  mmproj: null as string | null,
})
const createInstanceForm = ref({
  modelName: '',
  name: '',
  port: '',
  modelRef: '',
  mmproj: null as string | null,
})

const downloadForm = ref({
  repoId: '',
  patterns: '',
  localDir: '',
})

onMounted(async () => {
  await Promise.all([load(), loadDownloadTasks()])
  downloadPollTimer = window.setInterval(() => {
    void loadDownloadTasks()
  }, 1500)
})

onUnmounted(() => {
  if (downloadPollTimer) window.clearInterval(downloadPollTimer)
})

async function load() {
  const models = await listModelsHierarchy()
  rows.value = models.map((model) => ({
    ...model,
    filesCount: model.file_count ?? model.files.length,
    sizeText: humanSize(model.total_size_bytes ?? model.files.reduce((sum, file) => sum + file.size_bytes, 0)),
  }))
  if (selected.value) {
    selected.value = rows.value.find((item) => item.key === selected.value?.key) || null
  }
}

async function loadDownloadTasks() {
  const tasks = await listModelDownloadTasks().catch(() => [])
  downloadTasks.value = tasks
  const hasRunning = tasks.some((task) => task.running)
  if (hadRunningDownload && !hasRunning) {
    await load()
  }
  hadRunningDownload = hasRunning
}

async function download() {
  try {
    const task = await startModelDownloadTask({
      repo_id: downloadForm.value.repoId,
      patterns: parsePatterns(downloadForm.value.patterns),
      local_dir: downloadForm.value.localDir || null,
    })
    toast.add({
      severity: 'success',
      summary: 'Download started',
      detail: `${task.plan.family}/${task.plan.model}/${task.plan.quant}`,
      life: 2600,
    })
    await Promise.all([load(), loadDownloadTasks()])
  } catch (err) {
    toast.add({
      severity: 'error',
      summary: 'Download planning failed',
      detail: errorMessage(err),
      life: 4200,
    })
  }
}

function openRename(name: string) {
  renameForm.value = { current: name, next: name }
  showRename.value = true
}

async function rename() {
  await renameModel(renameForm.value.current, renameForm.value.next)
  toast.add({ severity: 'success', summary: 'Renamed', detail: `${renameForm.value.current} → ${renameForm.value.next}`, life: 2200 })
  showRename.value = false
  await load()
}

function openCreateFamily(modelKey: string) {
  const model = rows.value.find((item) => item.key === modelKey)
  if (!model) return
  const modelRef = modelRefFor(model, choosePrimaryGguf(model))
  const mmproj = modelRefFor(model, chooseMmproj(model))
  createFamilyForm.value = {
    modelName: model.key,
    name: `${sanitizeName(model.model)}-family`,
    family: model.family,
    description: `family from ${model.key}`,
    modelRef,
    mmproj,
  }
  showCreateFamily.value = true
}

function openCreateInstance(modelKey: string) {
  const model = rows.value.find((item) => item.key === modelKey)
  if (!model) return
  const modelRef = modelRefFor(model, choosePrimaryGguf(model))
  const mmproj = modelRefFor(model, chooseMmproj(model))
  createInstanceForm.value = {
    modelName: model.key,
    name: sanitizeName(model.model),
    port: '',
    modelRef,
    mmproj,
  }
  showCreateInstance.value = true
}

async function createFamilyFromModel() {
  await createTemplateFromModel({
    name: createFamilyForm.value.name,
    family: createFamilyForm.value.family,
    description: createFamilyForm.value.description,
    model_ref: createFamilyForm.value.modelRef,
    mmproj: createFamilyForm.value.mmproj,
  })
  toast.add({
    severity: 'success',
    summary: 'Family created',
    detail: createFamilyForm.value.name,
    life: 2200,
  })
  showCreateFamily.value = false
}

async function createInstanceFromModel() {
  await createInstance({
    name: createInstanceForm.value.name,
    model: createInstanceForm.value.modelRef,
    mmproj: createInstanceForm.value.mmproj,
    port: parsePort(createInstanceForm.value.port),
  })
  toast.add({
    severity: 'success',
    summary: 'Instance created',
    detail: createInstanceForm.value.name,
    life: 2200,
  })
  showCreateInstance.value = false
}

function confirmDelete(name: string) {
  confirm.require({
    message: `Delete model ${name}?`,
    acceptClass: 'p-button-danger',
    accept: async () => {
      await deleteModel(name)
      toast.add({ severity: 'success', summary: 'Deleted', detail: name, life: 2000 })
      await load()
    },
  })
}

function parsePatterns(raw: string) {
  const list = raw
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean)
  return list.length ? list : null
}

function parsePort(value: string) {
  const trimmed = value.trim()
  if (!trimmed) return null
  const parsed = Number(trimmed)
  return Number.isFinite(parsed) ? parsed : null
}

function choosePrimaryGguf(model: ModelHierarchyInfo) {
  const ggufs = model.files
    .filter((file) => file.name.toLowerCase().endsWith('.gguf') && !file.name.toLowerCase().includes('mmproj'))
    .slice()
    .sort((a, b) => {
      const ah = shardPriority(a.name)
      const bh = shardPriority(b.name)
      if (ah !== bh) return ah - bh
      return b.size_bytes - a.size_bytes
    })
  return ggufs[0] || null
}

function chooseMmproj(model: ModelHierarchyInfo) {
  return model.files.find((file) => file.name.toLowerCase().includes('mmproj') && file.name.toLowerCase().endsWith('.gguf')) || null
}

function shardPriority(name: string) {
  const lower = name.toLowerCase()
  if (lower.includes('-00001-of-')) return 0
  if (lower.includes('part1') || lower.includes('part-1')) return 1
  return 2
}

function modelRefFor(model: ModelHierarchyInfo, file: ModelFileInfo | null) {
  if (!file) return `/models/${model.key}`
  return `/models/${model.key}/${file.name}`
}

function sanitizeName(value: string) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

function humanSize(bytes: number) {
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let idx = 0
  while (value >= 1024 && idx < units.length - 1) {
    value /= 1024
    idx += 1
  }
  return `${value.toFixed(1)} ${units[idx]}`
}

function errorMessage(error: unknown) {
  if (typeof error === 'object' && error && 'response' in error) {
    const anyErr = error as any
    const message = anyErr?.response?.data?.error
    if (typeof message === 'string' && message.trim()) return message
  }
  if (error instanceof Error) return error.message
  return String(error ?? 'unknown error')
}
</script>
