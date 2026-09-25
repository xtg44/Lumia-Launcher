<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  player,
  bindAudio,
  togglePlay,
  playNext,
  playPrev,
  seek,
  setVolume,
  onTimeUpdate,
  onLoadedMetadata,
  onEnded,
  onAudioError,
  formatTime,
} from '../utils/player'

const { t } = useI18n()

// 常驻 <audio>：切换页面不销毁，音乐持续播放
const audioRef = ref<HTMLAudioElement | null>(null)

onMounted(() => {
  bindAudio(audioRef.value)
})
onUnmounted(() => {
  bindAudio(null)
})

function onSeek(e: Event) {
  const v = Number((e.target as HTMLInputElement).value)
  seek(v)
}

function onVolume(e: Event) {
  const v = Number((e.target as HTMLInputElement).value)
  setVolume(v)
}
</script>

<template>
  <div class="player-bar" :class="{ 'player-bar-hidden': !player.current }">
    <template v-if="player.current">
      <div class="player-info">
        <span class="player-song-name">{{ player.current.song.filename }}</span>
        <span class="player-playlist">{{ player.current.playlist }}</span>
      </div>

      <div class="player-controls">
        <button class="player-ctrl" @click="playPrev" :title="t('music.prev')">
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M6 6h2v12H6zM20 6v12l-9-6z"/></svg>
        </button>
        <button class="player-ctrl player-ctrl-main" @click="togglePlay">
          <svg v-if="player.playing" viewBox="0 0 24 24" fill="currentColor"><path d="M6 5h4v14H6zM14 5h4v14h-4z"/></svg>
          <svg v-else viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
        </button>
        <button class="player-ctrl" @click="playNext" :title="t('music.next')">
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M16 6h2v12h-2zM4 6v12l9-6z"/></svg>
        </button>
      </div>

      <div class="player-progress">
        <span class="player-time">{{ formatTime(player.currentTime) }}</span>
        <input
          class="player-range"
          type="range"
          min="0"
          :max="player.duration || 0"
          step="0.5"
          :value="player.currentTime"
          @input="onSeek"
        />
        <span class="player-time">{{ formatTime(player.duration) }}</span>
      </div>

      <div class="player-volume">
        <span>
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 9v6h4l5 5V4L7 9H3z"/></svg>
        </span>
        <input
          type="range"
          class="player-range player-volume-range"
          min="0"
          max="1"
          step="0.05"
          :value="player.volume"
          @input="onVolume"
        />
      </div>
    </template>

    <!-- audio 始终挂载，保证切页不断播 -->
    <audio
      ref="audioRef"
      @timeupdate="onTimeUpdate"
      @loadedmetadata="onLoadedMetadata"
      @ended="onEnded"
      @error="onAudioError"
    ></audio>
  </div>
</template>

<style scoped>
.player-bar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 18px;
  border-top: 1px solid var(--border-color);
  background: var(--header-bg);
  opacity: 1;
  transition: opacity 0.2s ease;
}

/* 无播放时整条隐藏（audio 仍保留，防切页断播） */
.player-bar-hidden {
  opacity: 0;
  pointer-events: none;
  max-height: 0;
  padding: 0 18px;
  overflow: hidden;
}

.player-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  max-width: 220px;
  flex-shrink: 1;
}

.player-song-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-color);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.player-playlist {
  font-size: 11px;
  opacity: 0.6;
  color: var(--text-color);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.player-controls {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.player-ctrl {
  border: none;
  background: transparent;
  color: var(--text-color);
  font-size: 15px;
  cursor: pointer;
  padding: 4px 6px;
  border-radius: 6px;
  opacity: 0.8;
  transition: opacity 0.15s ease;
}

.player-ctrl:hover {
  opacity: 1;
  background: rgba(128, 128, 128, 0.25);
}

.player-ctrl svg {
  width: 15px;
  height: 15px;
  display: block;
}

.player-ctrl-main svg {
  width: 18px;
  height: 18px;
}

.player-volume svg {
  width: 14px;
  height: 14px;
  display: block;
}

.player-ctrl-main {
  font-size: 18px;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  background: var(--accent-color);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 1;
}

.player-ctrl-main:hover {
  opacity: 0.9;
  background: var(--accent-color);
}

.player-progress {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.player-time {
  font-size: 11px;
  color: var(--text-color);
  opacity: 0.7;
  flex-shrink: 0;
}

.player-range {
  flex: 1;
  min-width: 0;
  accent-color: var(--accent-color);
}

.player-volume {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  font-size: 12px;
  opacity: 0.75;
}

.player-volume-range {
  width: 70px;
  flex: none;
}
</style>