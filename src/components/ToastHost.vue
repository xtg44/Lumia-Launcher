<script setup lang="ts">
import { useToasts } from '../utils/toast'

const { items } = useToasts()
</script>

<template>
  <div class="toast-host">
    <TransitionGroup name="toast">
      <div v-for="t in items" :key="t.id" class="toast-item" :class="`toast-${t.type}`">
        {{ t.message }}
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-host {
  position: fixed;
  top: 16px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 3000;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  pointer-events: none;
}
.toast-item {
  max-width: 70vw;
  padding: 10px 18px;
  border-radius: 10px;
  font-size: 13px;
  line-height: 1.4;
  color: #fff;
  background: rgba(42, 42, 54, 0.96);
  text-align: center;
  word-break: break-all;
}
.toast-success {
  background: rgba(46, 146, 80, 0.96);
}
.toast-error {
  background: rgba(200, 60, 80, 0.96);
}
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(-12px);
}
</style>