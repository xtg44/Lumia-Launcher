<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'

const props = defineProps<{
  modelValue: string
  options: { value: string; label: string }[]
  placeholder?: string
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: string): void
  (e: 'change', v: string): void
}>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)

const currentLabel = computed(
  () => props.options.find(o => o.value === props.modelValue)?.label ?? props.placeholder ?? ''
)

function choose(v: string) {
  emit('update:modelValue', v)
  emit('change', v)
  open.value = false
}

function onDocClick(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) open.value = false
}

onMounted(() => document.addEventListener('click', onDocClick))
onUnmounted(() => document.removeEventListener('click', onDocClick))
</script>

<template>
  <div ref="root" class="app-select" :class="{ open }">
    <button type="button" class="app-select-trigger" @click="open = !open">
      <span class="app-select-value">{{ currentLabel }}</span>
      <svg class="app-select-caret" width="12" height="12" viewBox="0 0 12 12">
        <path d="M2.5 4.5 6 8l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
    <Transition name="select-drop">
      <div v-if="open" class="app-select-menu">
        <button
          v-for="opt in options"
          :key="opt.value"
          type="button"
          class="app-select-option"
          :class="{ selected: opt.value === modelValue }"
          @click="choose(opt.value)"
        >
          {{ opt.label }}
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.app-select {
  position: relative;
  min-width: 130px;
}
.app-select-trigger {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 12px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 8px;
  background: var(--button-bg, #2d2d44);
  color: var(--text-color, #fff);
  font-size: 13px;
  cursor: pointer;
  transition: border-color 0.2s ease, background 0.2s ease;
}
.app-select-trigger:hover {
  border-color: rgba(255, 255, 255, 0.25);
}
.app-select-value {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.app-select-caret {
  color: var(--text-color, #fff);
  opacity: 0.55;
  flex-shrink: 0;
  transition: transform 0.2s ease;
}
.app-select.open .app-select-caret {
  transform: rotate(180deg);
}
.app-select-menu {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  z-index: 200;
  max-height: 260px;
  overflow-y: auto;
  padding: 4px;
  border-radius: 8px;
  background: var(--popup-bg, #ffffff);
  border: 1px solid var(--border-color);
  box-shadow: var(--popup-shadow);
}
.app-select-option {
  display: block;
  width: 100%;
  text-align: left;
  padding: 8px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-color, #fff);
  font-size: 13px;
  cursor: pointer;
}
.app-select-option:hover {
  background: var(--button-bg);
}
.app-select-option.selected {
  color: var(--accent-color, #6fa8ff);
}
.select-drop-enter-active,
.select-drop-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.select-drop-enter-from,
.select-drop-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>