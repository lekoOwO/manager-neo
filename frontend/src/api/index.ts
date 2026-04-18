import axios from 'axios'

const api = axios.create({ baseURL: '/api' })

export interface InstanceInfo {
  name: string
  display_name?: string
  path: string
  config: Record<string, any>
}

export interface InstanceHierarchyInfo {
  name: string
  display_name?: string
  family: string
  model: string
  quant: string
  variant: string
  model_ref: string
  mmproj_ref?: string
  path: string
  host_port: number
  config: Record<string, any>
}

export interface InstanceStatus {
  name: string
  status: string
  ports?: string
  error?: string
}

export interface ModelFileInfo {
  name: string
  path: string
  size_bytes: number
}

export interface ModelInfo {
  name: string
  path: string
  files: ModelFileInfo[]
}

export interface ModelHierarchyInfo {
  key: string
  family: string
  model: string
  quant: string
  path: string
  file_count: number
  total_size_bytes: number
  files: ModelFileInfo[]
}

export interface ModelDownloadPlan {
  repo_id: string
  patterns: string[]
  target_relative_dir: string
  target_absolute_dir: string
  script_path: string
  family: string
  model: string
  quant: string
  selected_files: string[]
  selected_mmproj_files: string[]
  selected_model_file: string
}

export interface ModelDownloadTaskStatus {
  id: string
  repo_id: string
  patterns: string[]
  phase: string
  running: boolean
  progress_percent: number
  latest_message: string
  error?: string
  output_path?: string
  started_at: number
  updated_at: number
  plan: ModelDownloadPlan
}

export interface TemplateInfo {
  name: string
  family: string
  description: string
  config: Record<string, any>
  overrides: Record<string, Record<string, any>>
}

export interface TemplateHierarchyInfo {
  name: string
  family: string
  model: string
  quant: string
  description: string
  variant_count: number
  variants: string[]
  config: Record<string, any>
  overrides: Record<string, Record<string, any>>
}

export interface GpuDeviceMetrics {
  id: string
  name?: string
  utilization_percent?: number
  memory_use_percent?: number
  temperature_c?: number
}

export interface SystemMetrics {
  unix_time: number
  cpu: {
    usage_percent: number
    cores: number
    load_1: number
    load_5: number
    load_15: number
  }
  ram: {
    total_mb: number
    used_mb: number
    free_mb: number
    available_mb: number
    usage_percent: number
  }
  rocm: {
    available: boolean
    devices: GpuDeviceMetrics[]
    raw: string
    error?: string
  }
}

export interface InstanceMemoryPreview {
  name: string
  model_ref: string
  gguf_path?: string
  architecture?: string
  model_bytes: number
  kv_cache_bytes: number
  overhead_bytes: number
  estimated_total_bytes: number
  context_size: number
  parallel: number
  cache_type_k: string
  cache_type_v: string
  warning?: string
}

export const listInstances = () => api.get<InstanceInfo[]>('/instances').then((r) => r.data)
export const listInstancesHierarchy = () =>
  api.get<InstanceHierarchyInfo[]>('/instances/hierarchy').then((r) => r.data)
export const getInstanceMemoryPreviews = () =>
  api.get<InstanceMemoryPreview[]>('/instances/memory-preview').then((r) => r.data)
export const createInstance = (payload: unknown) =>
  api.post('/instances', payload).then((r) => r.data)
export const deleteInstance = (name: string) =>
  api.delete(`/instances/${name}`).then((r) => r.data)
export const editInstance = (name: string, key: string, value: unknown) =>
  api.patch(`/instances/${name}`, { key, value }).then((r) => r.data)
export const startInstance = (name: string) =>
  api.post(`/instances/${name}/start`).then((r) => r.data)
export const stopInstance = (name: string) =>
  api.post(`/instances/${name}/stop`).then((r) => r.data)
export const restartInstance = (name: string) =>
  api.post(`/instances/${name}/restart`).then((r) => r.data)
export const listStatuses = () => api.get<InstanceStatus[]>('/status').then((r) => r.data)
export const getInstanceLogs = (name: string, tail = 200) =>
  api.get<{ logs: string }>(`/instances/${name}/logs`, { params: { tail } }).then((r) => r.data.logs)
export const getSystemMetrics = () =>
  api.get<SystemMetrics>('/system/metrics').then((r) => r.data)

export const listModels = () => api.get<ModelInfo[]>('/models').then((r) => r.data)
export const listModelsHierarchy = () =>
  api.get<ModelHierarchyInfo[]>('/models/hierarchy').then((r) => r.data)
export const planModelDownload = (payload: unknown) =>
  api.post<ModelDownloadPlan>('/models/download/plan', payload).then((r) => r.data)
export const downloadModel = (payload: unknown) =>
  api.post('/models/download', payload).then((r) => r.data)
export const startModelDownloadTask = (payload: unknown) =>
  api.post<ModelDownloadTaskStatus>('/models/download/tasks', payload).then((r) => r.data)
export const listModelDownloadTasks = () =>
  api.get<ModelDownloadTaskStatus[]>('/models/download/tasks').then((r) => r.data)
export const getModelDownloadTask = (id: string) =>
  api.get<ModelDownloadTaskStatus>(`/models/download/tasks/${id}`).then((r) => r.data)
export const deleteModel = (name: string) =>
  api.delete(`/models/${name}`).then((r) => r.data)
export const renameModel = (name: string, nextName: string) =>
  api.patch(`/models/${name}`, { name: nextName }).then((r) => r.data)

export const listTemplates = () => api.get<TemplateInfo[]>('/templates').then((r) => r.data)
export const listTemplatesHierarchy = () =>
  api.get<TemplateHierarchyInfo[]>('/templates/hierarchy').then((r) => r.data)
export const createTemplate = (payload: unknown) =>
  api.post('/templates', payload).then((r) => r.data)
export const createTemplateFromModel = (payload: unknown) =>
  api.post('/templates/from-model', payload).then((r) => r.data)
export const instantiateTemplate = (payload: unknown) =>
  api.post('/templates/instantiate', payload).then((r) => r.data)
export const batchApplyTemplate = (payload: unknown) =>
  api.post('/templates/batch-apply', payload).then((r) => r.data)
export const setTemplateOverride = (payload: unknown) =>
  api.post('/templates/set-override', payload).then((r) => r.data)
export const setTemplateBase = (payload: unknown) =>
  api.post('/templates/set-base', payload).then((r) => r.data)
export const deleteTemplate = (name: string) =>
  api.delete(`/templates/${name}`).then((r) => r.data)
export const scanTemplates = () => api.post('/templates/scan').then((r) => r.data)

export const getPortMap = () =>
  api.get<Record<string, string>>('/ports').then((r) => r.data)
