import { reactive } from 'vue'

export interface ToastItem {
  id: number
  message: string
  type: 'info' | 'success' | 'error'
}

const state = reactive<{ items: ToastItem[] }>({ items: [] })
let seq = 0

export function toast(message: string, type: ToastItem['type'] = 'info') {
  const id = ++seq
  state.items.push({ id, message, type })
  const duration = type === 'error' ? 6000 : 3500
  setTimeout(() => {
    const idx = state.items.findIndex(t => t.id === id)
    if (idx >= 0) state.items.splice(idx, 1)
  }, duration)
}

export function useToasts() {
  return state
}