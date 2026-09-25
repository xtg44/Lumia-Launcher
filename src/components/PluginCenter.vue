<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { pluginList, pluginSetEnabled, pluginDelete, pluginOpenDir } from '../utils/tauri'
import pluginSvgRaw from '../assets/icons/plugin.svg?raw'
import type { PluginInfo, PluginControlNode } from '../utils/tauri'
import { pluginUiTree } from '../utils/tauri'
import { useI18n } from 'vue-i18n'
import { confirmDialog } from '../utils/dialog'

const { t } = useI18n()

/** 通知父组件：启停/删除已变更，需刷新侧边栏注册表 */
const emit = defineEmits<{ changed: [] }>()

const plugins = ref<PluginInfo[]>([])
const loading = ref(true)
const error = ref('')
const busy = ref<Record<string, boolean>>({})

/** 每个插件可点击展开查看控件树预览 */
const expanded = ref<Record<string, boolean>>({})
const previews = ref<Record<string, PluginControlNode[]>>({})
const previewLoading = ref<Record<string, boolean>>({})

async function load() {
  loading.value = true
  error.value = ''
  try {
    plugins.value = await pluginList()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

/** 启停开关 */
async function toggleEnabled(p: PluginInfo) {
  busy.value[p.id] = true
  try {
    await pluginSetEnabled(p.id, !p.enabled)
    p.enabled = !p.enabled
    emit('changed')
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value[p.id] = false
  }
}

/** 删除插件（带确认） */
async function removePlugin(p: PluginInfo) {
  const msg = t('pluginCenter.confirmDelete', { name: p.name })
  if (!(await confirmDialog(msg))) return
  busy.value[p.id] = true
  try {
    await pluginDelete(p.id)
    plugins.value = plugins.value.filter((x) => x.id !== p.id)
    emit('changed')
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value[p.id] = false
  }
}

async function togglePreview(p: PluginInfo) {
  expanded.value[p.id] = !expanded.value[p.id]
  if (expanded.value[p.id] && !previews.value[p.id]) {
    previewLoading.value[p.id] = true
    try {
      previews.value[p.id] = await pluginUiTree(p.id)
    } catch (err) {
      previews.value[p.id] = []
      error.value = err instanceof Error ? err.message : String(err)
    } finally {
      previewLoading.value[p.id] = false
    }
  }
}

function iconUrl(p: PluginInfo): string {
  return p.icon ? convertFileSrc(p.icon) : 'data:image/svg+xml;utf8,' + encodeURIComponent(pluginSvgRaw)
}

function previewLabel(c: PluginControlNode): string {
  return c.text ? `${c.name} — ${c.text}` : c.name
}

onMounted(load)
</script>

<template>
  <div class="plugin-center">
    <div class="pc-head">
      <h2 class="pc-title">{{ t('pluginCenter.title') }}</h2>
      <div class="pc-actions">
        <button class="pc-btn" @click="load" :disabled="loading">{{ t('pluginCenter.refresh') }}</button>
      </div>
    </div>

    <div v-if="loading" class="pc-loading">{{ t('pluginCenter.loading') }}</div>
    <div v-else-if="error" class="pc-error">{{ error }}</div>

    <!-- 空状态：引导用户丢 .lplugin 进 plugins/ -->
    <div v-else-if="plugins.length === 0" class="pc-empty">
      <div class="pc-empty-title">{{ t('pluginCenter.noPlugins') }}</div>
      <div class="pc-empty-hint">{{ t('pluginCenter.emptyHint') }}</div>
      <button class="pc-btn" @click="load">{{ t('pluginCenter.refresh') }}</button>
    </div>

    <!-- 插件列表 -->
    <div v-else class="pc-list">
      <div v-for="p in plugins" :key="p.id" class="pc-card" :class="{ 'pc-disabled': !p.enabled }">
        <div class="pc-card-main">
          <img class="pc-card-icon" :src="iconUrl(p)" alt="" />
          <div class="pc-info">
            <div class="pc-name-row">
              <span class="pc-name">{{ p.name }}</span>
              <span v-if="p.builtin" class="pc-badge pc-badge-builtin">{{ t('pluginCenter.builtin') }}</span>
              <span class="pc-version">v{{ p.version || '—' }}</span>
            </div>
            <div class="pc-desc">{{ p.description || t('pluginCenter.noDesc') }}</div>
            <div class="pc-id">id: {{ p.id }}</div>
          </div>

          <div class="pc-ops">
            <label class="pc-toggle" :title="p.enabled ? t('pluginCenter.disable') : t('pluginCenter.enable')">
              <input type="checkbox" :checked="p.enabled" :disabled="busy[p.id]" @change="toggleEnabled(p)" />
              <span>{{ p.enabled ? t('pluginCenter.enabled') : t('pluginCenter.disabled') }}</span>
            </label>
            <button class="pc-btn" :disabled="busy[p.id]" @click="togglePreview(p)">
              {{ expanded[p.id] ? t('pluginCenter.collapse') : t('pluginCenter.preview') }}
            </button>
            <button class="pc-btn" :disabled="busy[p.id]" @click="pluginOpenDir(p.id)">
              {{ t('pluginCenter.openDir') }}
            </button>
            <button class="pc-btn pc-btn-danger" :disabled="busy[p.id]" @click="removePlugin(p)">
              {{ t('pluginCenter.delete') }}
            </button>
          </div>
        </div>

        <!-- 控件树预览（展开时） -->
        <div v-if="expanded[p.id]" class="pc-preview">
          <div v-if="previewLoading[p.id]" class="pc-preview-loading">{{ t('pluginCenter.loading') }}</div>
          <div v-else-if="previews[p.id] && previews[p.id].length === 0" class="pc-preview-empty">
            {{ t('pluginCenter.noControls') }}
          </div>
          <div v-else class="pc-preview-list">
            <div v-for="c in previews[p.id]" :key="c.name" class="pc-preview-item">
              {{ previewLabel(c) }}
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.plugin-center {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px 20px;
  height: 100%;
  overflow-y: auto;
}
.pc-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.pc-title {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
}
.pc-actions {
  display: flex;
  gap: 8px;
}
.pc-btn {
  padding: 6px 14px;
  border-radius: 7px;
  border: 1px solid var(--border, #333);
  background: transparent;
  color: inherit;
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
}
.pc-btn:hover {
  background: rgba(128, 128, 128, 0.12);
}
.pc-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.pc-btn-danger {
  border-color: rgba(229, 72, 77, 0.5);
  color: #e5484d;
}
.pc-btn-danger:hover {
  background: rgba(229, 72, 77, 0.12);
}
.pc-loading,
.pc-error {
  padding: 24px 8px;
  font-size: 13px;
  opacity: 0.7;
}
.pc-error {
  color: #e5484d;
  opacity: 1;
}
.pc-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 48px 20px;
  text-align: center;
}
.pc-empty-title {
  font-size: 15px;
  font-weight: 600;
}
.pc-empty-hint {
  font-size: 13px;
  opacity: 0.65;
  max-width: 420px;
  line-height: 1.5;
}
.pc-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.pc-card {
  border: 1px solid var(--border, #333);
  border-radius: 10px;
  padding: 12px 14px;
  background: var(--bg-card, rgba(255, 255, 255, 0.03));
}
.pc-card-icon {
  width: 40px;
  height: 40px;
  border-radius: 10px;
  object-fit: contain;
  flex-shrink: 0;
  background: rgba(255, 255, 255, 0.06);
  padding: 6px;
}
.pc-card.pc-disabled {
  opacity: 0.55;
}
.pc-card-main {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.pc-info {
  flex: 1;
  min-width: 0;
}
.pc-name-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex-wrap: wrap;
}
.pc-name {
  font-size: 15px;
  font-weight: 600;
}
.pc-badge {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 4px;
}
.pc-badge-builtin {
  background: rgba(79, 140, 255, 0.18);
  color: #7fb0ff;
}
.pc-version {
  font-size: 12px;
  opacity: 0.6;
}
.pc-desc {
  font-size: 13px;
  opacity: 0.75;
  margin-top: 4px;
  line-height: 1.4;
}
.pc-id {
  font-size: 11px;
  opacity: 0.45;
  margin-top: 4px;
  font-family: ui-monospace, monospace;
}
.pc-ops {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 8px;
}
.pc-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  cursor: pointer;
}
.pc-preview {
  margin-top: 10px;
  border-top: 1px dashed var(--border, #333);
  padding-top: 10px;
}
.pc-preview-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.pc-preview-item {
  font-size: 12px;
  opacity: 0.8;
  padding-left: 8px;
}
.pc-preview-loading,
.pc-preview-empty {
  font-size: 12px;
  opacity: 0.6;
}
</style>