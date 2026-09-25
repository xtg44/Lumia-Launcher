<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useConfirmDialog, resolveConfirm } from '../utils/dialog'

const { t } = useI18n()
const { visible, message } = useConfirmDialog()
</script>

<template>
  <div v-if="visible" class="confirm-overlay" @click.self="resolveConfirm(false)">
    <div class="confirm-panel" role="dialog" aria-modal="true">
      <div class="confirm-message">{{ message }}</div>
      <div class="confirm-actions">
        <button class="confirm-btn ghost" @click="resolveConfirm(false)">{{ t('common.cancel') }}</button>
        <button class="confirm-btn danger" @click="resolveConfirm(true)">{{ t('common.confirm') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.confirm-overlay {
  position: fixed;
  inset: 0;
  z-index: 4000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.4);
}
.confirm-panel {
  min-width: 300px;
  max-width: 70vw;
  padding: 20px 22px;
  border-radius: 14px;
  background: var(--popup-bg, #ffffff);
  border: 1px solid var(--border-color);
  box-shadow: var(--popup-shadow);
}
.confirm-message {
  font-size: 14px;
  line-height: 1.5;
  color: var(--text-color, #fff);
  word-break: break-all;
}
.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
}
.confirm-btn {
  padding: 8px 18px;
  border: none;
  border-radius: 8px;
  font-size: 13px;
  cursor: pointer;
  transition: opacity 0.2s ease;
}
.confirm-btn:hover {
  opacity: 0.85;
}
.confirm-btn.ghost {
  background: var(--button-bg, #2d2d44);
  color: var(--text-color, #fff);
}
.confirm-btn.danger {
  background: #e94560;
  color: #fff;
}
</style>