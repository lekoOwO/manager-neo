<template>
  <div class="space-y-3">
    <Card class="panel-corner">
      <template #title><span class="section-title">Family Template Operations</span></template>
      <template #content>
        <div class="grid grid-cols-1 lg:grid-cols-4 gap-2">
          <InputText v-model="createForm.name" placeholder="template name" class="mono-data" />
          <InputText v-model="createForm.family" placeholder="family (optional)" class="mono-data" />
          <InputText v-model="createForm.description" placeholder="description" class="mono-data" />
          <InputText v-model="createForm.fromInstance" placeholder="from instance (optional)" class="mono-data" />
        </div>
        <div class="mt-2 flex gap-2 flex-wrap">
          <Button label="Create" icon="pi pi-plus" @click="create" />
          <Button label="Scan Families" icon="pi pi-search" severity="secondary" @click="scan" />
          <Button label="Refresh" icon="pi pi-refresh" severity="secondary" @click="load" />
        </div>
      </template>
    </Card>

    <div class="space-y-3">
      <Card class="panel-corner">
        <template #title><span class="section-title">Family List</span></template>
        <template #content>
          <DataTable
            :value="rows"
            data-key="name"
            size="small"
            selection-mode="single"
            v-model:selection="selected"
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
            <Column header="Template">
              <template #body="{ data }">
                <span class="mono-data block break-all" :title="data.name">{{ data.name }}</span>
              </template>
            </Column>
            <Column field="variants" header="Variants" />
            <Column header="Ops">
              <template #body="{ data }">
                <Button icon="pi pi-trash" size="small" severity="danger" @click="confirmDelete(data.name)" />
              </template>
            </Column>
          </DataTable>
        </template>
      </Card>

      <Card class="panel-corner">
        <template #title><span class="section-title">Template Detail + Diff Editing</span></template>
        <template #content>
          <div v-if="selectedTemplate" class="space-y-3">
            <TabMenu :model="tabItems" v-model:activeIndex="activeTab" />

            <div v-if="activeTab === 0" class="space-y-3">
              <div class="flex gap-2 flex-wrap">
                <Button label="Instantiate" icon="pi pi-copy" @click="openInstantiate()" />
                <Button label="Batch Apply" icon="pi pi-sliders-h" severity="warn" @click="openBatch()" />
                <Button label="Edit Base Param" icon="pi pi-cog" class="action-edit" @click="openBaseEdit()" />
              </div>

              <div class="grid grid-cols-1 md:grid-cols-2 gap-2 mono-data text-[11px]">
                <div class="param-block">
                  <div class="param-title">Runtime</div>
                  <div class="param-row"><span>Family</span><span>{{ selectedTemplate.family || '-' }}</span></div>
                  <div class="param-row"><span>Model</span><span>{{ selectedTemplate.model || '-' }}</span></div>
                  <div class="param-row"><span>Quant</span><span>{{ selectedTemplate.quant || '-' }}</span></div>
                  <div class="param-row"><span>Template</span><span>{{ selectedTemplate.name }}</span></div>
                  <div class="param-row"><span>Model Ref</span><span>{{ selectedTemplate.config.model || '-' }}</span></div>
                  <div class="param-row"><span>MMProj</span><span>{{ selectedTemplate.config.mmproj || '-' }}</span></div>
                </div>
                <div class="param-block">
                  <div class="param-title">Execution</div>
                  <div class="param-row"><span>Ctx Size</span><span>{{ selectedTemplate.config.ctx_size ?? '-' }}</span></div>
                  <div class="param-row"><span>Threads</span><span>{{ selectedTemplate.config.threads ?? '-' }}</span></div>
                  <div class="param-row"><span>Parallel</span><span>{{ selectedTemplate.config.parallel ?? '-' }}</span></div>
                  <div class="param-row"><span>GPU Layers</span><span>{{ selectedTemplate.config.n_gpu_layers ?? selectedTemplate.config.gpu_layers ?? '-' }}</span></div>
                </div>
                <div class="param-block">
                  <div class="param-title">Sampling</div>
                  <div class="param-row"><span>Temp</span><span>{{ selectedTemplate.config.sampling?.temp ?? '-' }}</span></div>
                  <div class="param-row"><span>Top-P</span><span>{{ selectedTemplate.config.sampling?.top_p ?? '-' }}</span></div>
                  <div class="param-row"><span>Top-K</span><span>{{ selectedTemplate.config.sampling?.top_k ?? '-' }}</span></div>
                  <div class="param-row"><span>Min-P</span><span>{{ selectedTemplate.config.sampling?.min_p ?? '-' }}</span></div>
                </div>
                <div class="param-block">
                  <div class="param-title">Template Meta</div>
                  <div class="param-row"><span>Description</span><span>{{ selectedTemplate.description || '-' }}</span></div>
                  <div class="param-row"><span>Variants</span><span>{{ variantRows.length }}</span></div>
                </div>
              </div>

              <DataTable :value="baseEntries" data-key="key" size="small">
                <Column header="Source">
                  <template #body="{ data }">
                    <span class="mono-data text-[10px] text-cyan-300">{{ data.sourceLabel }}</span>
                  </template>
                </Column>
                <Column field="key" header="Base Key Path" />
                <Column field="type" header="Type" />
                <Column field="value" header="Value" />
              </DataTable>
            </div>

            <div v-else class="space-y-3">
              <div class="flex gap-2 flex-wrap">
                <Button
                  label="Edit Selected Override"
                  icon="pi pi-pen-to-square"
                  class="action-edit"
                  :disabled="!selectedVariantName"
                  @click="selectedVariantName && openOverrideEdit(selectedVariantName)"
                />
              </div>
              <div class="grid grid-cols-1 lg:grid-cols-12 gap-2">
                <div class="lg:col-span-5">
                  <DataTable
                    :value="variantRows"
                    data-key="name"
                    size="small"
                    selection-mode="single"
                    v-model:selection="selectedVariantRow"
                  >
                    <Column field="name" header="Variant" />
                    <Column field="changes" header="Change Count" />
                    <Column header="Ops">
                      <template #body="{ data }">
                        <Button label="Edit" size="small" class="action-edit" @click="openOverrideEdit(data.name)" />
                      </template>
                    </Column>
                  </DataTable>
                </div>
                <div class="lg:col-span-7 space-y-2">
                  <Card class="panel-corner">
                    <template #title><span class="section-title">Variant Diff Detail</span></template>
                    <template #content>
                      <div v-if="selectedVariantName" class="mono-data text-[11px] text-zinc-400 mb-2">
                        Variant: {{ selectedVariantName }}
                      </div>
                      <DataTable :value="selectedVariantEntries" size="small" data-key="key">
                        <Column header="Source">
                          <template #body="{ data }">
                            <span class="mono-data text-[10px] text-cyan-300">{{ data.sourceLabel }}</span>
                          </template>
                        </Column>
                        <Column field="key" header="Key Path" />
                        <Column field="type" header="Type" />
                        <Column field="value" header="Override Value" />
                      </DataTable>
                    </template>
                  </Card>
                  <Card class="panel-corner">
                    <template #title><span class="section-title">Editing Guide</span></template>
                    <template #content>
                      <div class="mono-data text-[11px] space-y-1 text-zinc-300">
                        <div>1. Pick key path (e.g. llama.sampling.temp / llama.n_gpu_layers / compose.host_port)</div>
                        <div>2. Select value mode (number / boolean / json / string)</div>
                        <div>3. Enter value using matching type</div>
                        <div>4. Submit and verify changed row in diff table</div>
                      </div>
                    </template>
                  </Card>
                </div>
              </div>
            </div>
          </div>
          <div v-else class="mono-data text-zinc-500 text-sm">Select a template to inspect base parameters and overrides.</div>
        </template>
      </Card>
    </div>

    <Dialog v-model:visible="showInstantiate" modal header="Instantiate from Family Template" :style="{ width: '36rem' }">
      <div class="space-y-2">
        <InputText v-model="instantiateForm.templateName" disabled />
        <InputText v-model="instantiateForm.instanceName" placeholder="new instance name" class="mono-data" />
        <Textarea v-model="instantiateForm.overrides" rows="4" placeholder='overrides JSON object (optional), e.g. {"sampling.temp":0.7}' class="mono-data w-full" />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showInstantiate = false" />
        <Button label="Create" @click="instantiate" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showBatch" modal header="Batch Apply to Family" :style="{ width: '38rem' }">
      <div class="space-y-2">
        <InputText v-model="batchForm.templateName" disabled />
        <AutoComplete
          v-model="batchForm.key"
          :suggestions="batchKeySuggestions"
          dropdown
          class="w-full mono-data"
          placeholder="key path, e.g. llama.sampling.temp"
          @complete="completeBatchKey"
        />
        <Select v-model="batchForm.mode" :options="valueModes" option-label="label" option-value="value" />
        <Select v-if="batchForm.mode === 'boolean'" v-model="batchForm.boolValue" :options="boolOptions" option-label="label" option-value="value" />
        <Textarea v-else-if="batchForm.mode === 'json'" v-model="batchForm.raw" rows="4" placeholder='{"nested":"json"}' class="mono-data w-full" />
        <InputText v-else v-model="batchForm.raw" placeholder="value" class="mono-data" />
        <div class="mono-data text-[11px] text-zinc-400">Examples: number=0.7 / boolean=true / json={"x":1} / string=Q8_0</div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showBatch = false" />
        <Button label="Apply" @click="batchApply" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showBaseEdit" modal header="Edit Unified Base Parameter" :style="{ width: '38rem' }">
      <div class="space-y-2">
        <InputText v-model="baseForm.templateName" disabled />
        <AutoComplete
          v-model="baseForm.key"
          :suggestions="baseKeySuggestions"
          dropdown
          class="w-full mono-data"
          placeholder="key path, e.g. llama.ctx_size"
          @complete="completeBaseKey"
        />
        <Select v-model="baseForm.mode" :options="valueModes" option-label="label" option-value="value" />
        <Select v-if="baseForm.mode === 'boolean'" v-model="baseForm.boolValue" :options="boolOptions" option-label="label" option-value="value" />
        <Textarea v-else-if="baseForm.mode === 'json'" v-model="baseForm.raw" rows="4" placeholder='{"nested":"json"}' class="mono-data w-full" />
        <InputText v-else v-model="baseForm.raw" placeholder="value" class="mono-data" />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showBaseEdit = false" />
        <Button label="Apply" @click="applyBaseEdit" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showOverrideEdit" modal header="Edit Variant Override" :style="{ width: '42rem' }">
      <div class="space-y-2">
        <InputText v-model="overrideForm.templateName" disabled />
        <InputText v-model="overrideForm.variantName" disabled />
        <AutoComplete
          v-model="overrideForm.key"
          :suggestions="overrideKeySuggestions"
          dropdown
          class="w-full mono-data"
          placeholder="key path, e.g. llama.sampling.temp"
          @complete="completeOverrideKey"
        />
        <div class="flex flex-wrap gap-1">
          <Button
            v-for="key in suggestedKeys"
            :key="key"
            :label="key"
            size="small"
            severity="secondary"
            @click="overrideForm.key = key"
          />
        </div>
        <Select v-model="overrideForm.mode" :options="valueModes" option-label="label" option-value="value" />
        <Select v-if="overrideForm.mode === 'boolean'" v-model="overrideForm.boolValue" :options="boolOptions" option-label="label" option-value="value" />
        <Textarea v-else-if="overrideForm.mode === 'json'" v-model="overrideForm.raw" rows="5" placeholder='{"nested":"json"}' class="mono-data w-full" />
        <InputText v-else v-model="overrideForm.raw" placeholder="value" class="mono-data" />
        <div class="mono-data text-[11px] text-zinc-400">
          Suggested value examples: sampling.temp=0.7, n_gpu_layers=999, thinking=false, extra={"kv":"json"}.
        </div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" @click="showOverrideEdit = false" />
        <Button label="Apply" @click="applyOverrideEdit" />
      </template>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useConfirm } from 'primevue/useconfirm'
import { useToast } from 'primevue/usetoast'
import AutoComplete from 'primevue/autocomplete'
import Button from 'primevue/button'
import Card from 'primevue/card'
import Column from 'primevue/column'
import DataTable from 'primevue/datatable'
import Dialog from 'primevue/dialog'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import TabMenu from 'primevue/tabmenu'
import Textarea from 'primevue/textarea'

import {
  batchApplyTemplate,
  createTemplate,
  deleteTemplate,
  instantiateTemplate,
  listTemplatesHierarchy,
  scanTemplates,
  setTemplateBase,
  setTemplateOverride,
  type TemplateHierarchyInfo,
} from '@/api'

type ValueMode = 'auto' | 'string' | 'number' | 'boolean' | 'json'
type KeySource = 'llama' | 'compose' | 'meta' | 'other'

interface FlatEntry {
  key: string
  rawKey: string
  source: KeySource
  sourceLabel: string
  type: string
  value: string
}

const toast = useToast()
const confirm = useConfirm()

const templates = ref<TemplateHierarchyInfo[]>([])
const selected = ref<{ name: string; family: string; model: string; quant: string; variants: number } | null>(null)
const selectedVariantName = ref('')
const selectedVariantRow = ref<{ name: string; changes: number } | null>(null)
const activeTab = ref(0)

const tabItems = [{ label: 'Base Parameters' }, { label: 'Variant Diffs' }]

const valueModes = [
  { label: 'Auto (JSON then string)', value: 'auto' },
  { label: 'String', value: 'string' },
  { label: 'Number', value: 'number' },
  { label: 'Boolean', value: 'boolean' },
  { label: 'JSON', value: 'json' },
]
const boolOptions = [
  { label: 'true', value: true },
  { label: 'false', value: false },
]

const LLAMA_ROOTS = new Set([
  'model',
  'mmproj',
  'draft_model',
  'draft_max',
  'draft_min',
  'chat_template_file',
  'ctx_size',
  'parallel',
  'cont_batching',
  'cache_type_k',
  'cache_type_v',
  'n_gpu_layers',
  'gpu_layers',
  'threads',
  'threads_batch',
  'flash_attn',
  'no_mmap',
  'embedding',
  'reranking',
  'pooling',
  'sampling',
  'batch',
  'rope',
  'chat_template_kwargs',
])
const COMPOSE_ROOTS = new Set([
  'host',
  'port',
  'host_port',
  'docker_image',
  'volumes_ro',
  'ipc_host',
  'memory_limit',
  'environment',
  'healthcheck',
  'logging',
  'restart',
  'service_name',
  'extra_volumes',
  'container_name',
])
const META_ROOTS = new Set(['name'])
const SOURCE_LABELS: Record<KeySource, string> = {
  llama: 'LLAMA',
  compose: 'COMPOSE',
  meta: 'META',
  other: 'OTHER',
}

const createForm = ref({
  name: '',
  family: '',
  description: '',
  fromInstance: '',
})

const showInstantiate = ref(false)
const showBatch = ref(false)
const showBaseEdit = ref(false)
const showOverrideEdit = ref(false)
const overrideKeySuggestions = ref<string[]>([])
const baseKeySuggestions = ref<string[]>([])
const batchKeySuggestions = ref<string[]>([])

const instantiateForm = ref({
  templateName: '',
  instanceName: '',
  overrides: '',
})

const batchForm = ref({
  templateName: '',
  key: '',
  mode: 'auto' as ValueMode,
  raw: '',
  boolValue: true,
})

const baseForm = ref({
  templateName: '',
  key: '',
  mode: 'auto' as ValueMode,
  raw: '',
  boolValue: true,
})

const overrideForm = ref({
  templateName: '',
  variantName: '',
  key: '',
  mode: 'auto' as ValueMode,
  raw: '',
  boolValue: true,
})

const rows = computed(() =>
  templates.value.map((template) => ({
    name: template.name,
    family: template.family,
    model: template.model,
    quant: template.quant,
    variants: template.variant_count ?? Object.keys(template.overrides || {}).length,
  })),
)

const selectedTemplate = computed(() =>
  templates.value.find((template) => template.name === selected.value?.name),
)

const baseEntries = computed(() => flattenObject(selectedTemplate.value?.config || {}))

const variantRows = computed(() => {
  const template = selectedTemplate.value
  if (!template) return []
  return Object.entries(template.overrides || {}).map(([name, diff]) => ({
    name,
    changes: Object.keys(diff || {}).length,
  }))
})

const selectedVariantEntries = computed(() => {
  const template = selectedTemplate.value
  if (!template || !selectedVariantName.value) return []
  return flattenObject(template.overrides[selectedVariantName.value] || {})
})

const suggestedKeys = computed(() => baseEntries.value.map((entry) => entry.key).slice(0, 10))

onMounted(load)

watch(selectedTemplate, (next) => {
  if (!next) {
    selectedVariantName.value = ''
    selectedVariantRow.value = null
    return
  }
  const first = Object.keys(next.overrides || {})[0] || ''
  selectedVariantName.value = first
  selectedVariantRow.value = first ? { name: first, changes: Object.keys(next.overrides[first] || {}).length } : null
})

watch(selectedVariantRow, (next) => {
  selectedVariantName.value = next?.name || ''
})

async function load() {
  templates.value = await listTemplatesHierarchy()
  if (selected.value) {
    selected.value = rows.value.find((item) => item.name === selected.value?.name) || null
  }
}

async function create() {
  await createTemplate({
    name: createForm.value.name,
    family: createForm.value.family || null,
    description: createForm.value.description || '',
    from_instance: createForm.value.fromInstance || null,
  })
  toast.add({ severity: 'success', summary: 'Template created', detail: createForm.value.name, life: 2200 })
  createForm.value = { name: '', family: '', description: '', fromInstance: '' }
  await load()
}

async function scan() {
  await scanTemplates()
  toast.add({ severity: 'info', summary: 'Scan complete', life: 1800 })
  await load()
}

function openInstantiate() {
  if (!selectedTemplate.value) return
  instantiateForm.value = {
    templateName: selectedTemplate.value.name,
    instanceName: '',
    overrides: '',
  }
  showInstantiate.value = true
}

async function instantiate() {
  await instantiateTemplate({
    template_name: instantiateForm.value.templateName,
    instance_name: instantiateForm.value.instanceName,
    overrides: instantiateForm.value.overrides ? parseObjectValue(instantiateForm.value.overrides) : null,
  })
  toast.add({ severity: 'success', summary: 'Instance created', detail: instantiateForm.value.instanceName, life: 2200 })
  showInstantiate.value = false
}

function openBatch() {
  if (!selectedTemplate.value) return
  batchForm.value = {
    templateName: selectedTemplate.value.name,
    key: '',
    mode: 'auto',
    raw: '',
    boolValue: true,
  }
  batchKeySuggestions.value = filterKeySuggestions('')
  showBatch.value = true
}

async function batchApply() {
  await batchApplyTemplate({
    template_name: batchForm.value.templateName,
    key: batchForm.value.key,
    value: resolveModeValue(batchForm.value.mode, batchForm.value.raw, batchForm.value.boolValue),
  })
  toast.add({ severity: 'success', summary: 'Batch applied', detail: batchForm.value.key, life: 2200 })
  showBatch.value = false
  await load()
}

function openBaseEdit() {
  if (!selectedTemplate.value) return
  baseForm.value = {
    templateName: selectedTemplate.value.name,
    key: '',
    mode: 'auto',
    raw: '',
    boolValue: true,
  }
  baseKeySuggestions.value = filterKeySuggestions('')
  showBaseEdit.value = true
}

async function applyBaseEdit() {
  await setTemplateBase({
    template_name: baseForm.value.templateName,
    key: baseForm.value.key,
    value: resolveModeValue(baseForm.value.mode, baseForm.value.raw, baseForm.value.boolValue),
  })
  toast.add({ severity: 'success', summary: 'Base updated', detail: baseForm.value.key, life: 2200 })
  showBaseEdit.value = false
  await load()
}

function openOverrideEdit(variantName: string) {
  if (!selectedTemplate.value) return
  selectedVariantName.value = variantName
  overrideForm.value = {
    templateName: selectedTemplate.value.name,
    variantName,
    key: '',
    mode: 'auto',
    raw: '',
    boolValue: true,
  }
  overrideKeySuggestions.value = filterKeySuggestions('')
  showOverrideEdit.value = true
}

async function applyOverrideEdit() {
  await setTemplateOverride({
    template_name: overrideForm.value.templateName,
    variant_name: overrideForm.value.variantName,
    key: overrideForm.value.key,
    value: resolveModeValue(overrideForm.value.mode, overrideForm.value.raw, overrideForm.value.boolValue),
  })
  toast.add({ severity: 'success', summary: 'Override updated', detail: `${overrideForm.value.variantName}.${overrideForm.value.key}`, life: 2200 })
  showOverrideEdit.value = false
  await load()
}

function confirmDelete(name: string) {
  confirm.require({
    message: `Delete template ${name}?`,
    acceptClass: 'p-button-danger',
    accept: async () => {
      await deleteTemplate(name)
      toast.add({ severity: 'success', summary: 'Deleted', detail: name, life: 1800 })
      await load()
    },
  })
}

function parseValue(raw: string): unknown {
  try {
    return JSON.parse(raw)
  } catch {
    return raw
  }
}

function parseObjectValue(raw: string): Record<string, unknown> {
  const parsed = parseValue(raw)
  if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
    return parsed as Record<string, unknown>
  }
  throw new Error('Overrides must be JSON object')
}

function resolveModeValue(mode: ValueMode, raw: string, boolValue: boolean): unknown {
  if (mode === 'boolean') return boolValue
  if (mode === 'string') return raw
  if (mode === 'number') {
    const number = Number(raw)
    if (Number.isNaN(number)) throw new Error('Value is not a valid number')
    return number
  }
  if (mode === 'json') return JSON.parse(raw)
  return parseValue(raw)
}

function normalizeKeyPath(path: string) {
  const trimmed = path.trim()
  if (!trimmed) return ''
  for (const prefix of ['llama.', 'compose.', 'meta.', 'other.']) {
    if (trimmed.startsWith(prefix)) {
      const rest = trimmed.slice(prefix.length).trim()
      return rest || trimmed
    }
  }
  return trimmed
}

function keySourceOf(path: string): KeySource {
  const normalized = normalizeKeyPath(path)
  const root = normalized.split('.')[0]?.toLowerCase() || ''
  if (LLAMA_ROOTS.has(root)) return 'llama'
  if (COMPOSE_ROOTS.has(root)) return 'compose'
  if (META_ROOTS.has(root)) return 'meta'
  return 'other'
}

function displayKey(path: string) {
  const normalized = normalizeKeyPath(path)
  if (!normalized) return '(root)'
  return `${keySourceOf(normalized)}.${normalized}`
}

function flattenObject(value: unknown, prefix = ''): FlatEntry[] {
  if (value === null) {
    const rawKey = prefix || '(root)'
    const source = keySourceOf(rawKey)
    return [
      {
        key: displayKey(rawKey),
        rawKey,
        source,
        sourceLabel: SOURCE_LABELS[source],
        type: 'null',
        value: 'null',
      },
    ]
  }
  if (Array.isArray(value)) {
    const rawKey = prefix || '(root)'
    const source = keySourceOf(rawKey)
    return [
      {
        key: displayKey(rawKey),
        rawKey,
        source,
        sourceLabel: SOURCE_LABELS[source],
        type: 'array',
        value: JSON.stringify(value),
      },
    ]
  }
  if (typeof value === 'object') {
    const obj = value as Record<string, unknown>
    return Object.entries(obj).flatMap(([key, next]) => {
      const path = prefix ? `${prefix}.${key}` : key
      if (next && typeof next === 'object' && !Array.isArray(next)) {
        return flattenObject(next, path)
      }
      return [
        {
          key: displayKey(path),
          rawKey: path,
          source: keySourceOf(path),
          sourceLabel: SOURCE_LABELS[keySourceOf(path)],
          type: Array.isArray(next) ? 'array' : next === null ? 'null' : typeof next,
          value: typeof next === 'string' ? next : JSON.stringify(next),
        },
      ]
    })
  }
  const rawKey = prefix || '(root)'
  const source = keySourceOf(rawKey)
  return [
    {
      key: displayKey(rawKey),
      rawKey,
      source,
      sourceLabel: SOURCE_LABELS[source],
      type: typeof value,
      value: String(value),
    },
  ]
}

function completeOverrideKey(event: { query: string }) {
  overrideKeySuggestions.value = filterKeySuggestions(event.query)
}

function completeBaseKey(event: { query: string }) {
  baseKeySuggestions.value = filterKeySuggestions(event.query)
}

function completeBatchKey(event: { query: string }) {
  batchKeySuggestions.value = filterKeySuggestions(event.query)
}

function filterKeySuggestions(query: string) {
  const term = query.trim().toLowerCase()
  const keys = baseEntries.value.map((entry) => entry.key)
  if (!term) return keys.slice(0, 20)
  return baseEntries.value
    .filter((entry) => entry.key.toLowerCase().includes(term) || entry.rawKey.toLowerCase().includes(term))
    .map((entry) => entry.key)
    .slice(0, 20)
}
</script>
