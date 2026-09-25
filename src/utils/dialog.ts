import { reactive } from 'vue'

const state = reactive({
  visible: false,
  message: '',
  resolve: null as null | ((ok: boolean) => void)
})

export function confirmDialog(message: string): Promise<boolean> {
  return new Promise(resolve => {
    state.message = message
    state.resolve = resolve
    state.visible = true
  })
}

export function resolveConfirm(ok: boolean) {
  const r = state.resolve
  state.resolve = null
  state.visible = false
  if (r) r(ok)
}

export function useConfirmDialog() {
  return state
}