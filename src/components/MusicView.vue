<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import {
  musicImportPaths,
  musicImportFolder,
  musicList,
  musicDeletePlaylist,
  musicRemoveSong,
} from '../utils/tauri'
import type { MusicPlaylist, MusicSong } from '../utils/tauri'
import { useI18n } from 'vue-i18n'
import { confirmDialog } from '../utils/dialog'

const { t } = useI18n()

// ===== 歌单 =====
const playlists = ref<MusicPlaylist[]>([])
const currentPlaylistName = ref('')
const loading = ref(true)
const error = ref('')

const currentPlaylist = computed(() =>
  playlists.value.find((p) => p.name === currentPlaylistName.value) || null,
)

async function loadPlaylists() {
  error.value = ''
  try {
    playlists.value = await musicList()
    if (!currentPlaylist.value && playlists.value.length > 0) {
      currentPlaylistName.value = playlists.value[0].name
    }
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

// ===== 导入 =====
import { open } from '@tauri-apps/plugin-dialog'
const importing = ref(false)
const importMsg = ref('')

/** 系统文件选择器允许的文件类型（音频 + 视频，视频只播音轨） */
const MEDIA_EXTENSIONS = [
  'mp3', 'flac', 'wav', 'm4a', 'aac', 'ogg', 'opus', 'wma', 'aiff',
  'mp4', 'm4v', 'mov', 'webm', 'ogv', 'mkv',
]

/** 导入音频/视频文件：归入当前歌单；未选中歌单时归入固定「我的音乐」歌单 */
async function importFiles() {
  const selected = await open({
    multiple: true,
    filters: [{ name: t('music.mediaFilter'), extensions: MEDIA_EXTENSIONS }],
  })
  if (!selected) return
  const paths = Array.isArray(selected) ? selected : [selected]
  const playlist = currentPlaylist.value ? currentPlaylist.value.name : t('music.defaultPlaylist')
  await runImport(async () => {
    return await musicImportPaths(playlist, paths)
  })
}

/** 导入文件夹为歌单（歌单名 = 文件夹名，后端自动按名字建歌单） */
async function importFolder() {
  const dir = await open({ directory: true })
  if (!dir) return
  await runImport(async () => {
    return await musicImportFolder(dir)
  })
}

async function runImport(doImport: () => Promise<{ imported: number; skipped: number }>) {
  importing.value = true
  importMsg.value = t('music.importingNow')
  try {
    const result = await doImport()
    const { imported, skipped } = result
    await loadPlaylists()
    if (imported === 0 && skipped === 0) {
      importMsg.value = t('music.importEmpty')
    } else if (skipped > 0) {
      importMsg.value = t('music.importDoneSkipped', { ok: String(imported), skipped: String(skipped) })
    } else {
      importMsg.value = t('music.importDone', { count: String(imported) })
    }
  } catch (err) {
    importMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
  }
  setTimeout(() => { importMsg.value = '' }, 5000)
}

// ===== 删除 =====
const deleting = ref(false)

async function onDeletePlaylist(name: string) {
  if (!(await confirmDialog(t('music.confirmDeletePlaylist', { name })))) return
  deleting.value = true
  try {
    await musicDeletePlaylist(name)
    if (currentPlaylistName.value === name) currentPlaylistName.value = ''
    await loadPlaylists()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    deleting.value = false
  }
}

async function onRemoveSong(song: MusicSong) {
  if (!currentPlaylist.value) return
  if (!(await confirmDialog(t('music.confirmRemoveSong', { name: song.filename })))) return
  deleting.value = true
  try {
    await musicRemoveSong(currentPlaylist.value.name, song.filename)
    // 若移除的正是当前播放的歌，停止全局播放并清空
    if (
      player.current
      && player.current.song.filename === song.filename
      && player.current.playlist === currentPlaylist.value.name
    ) {
      player.current = null
      player.playing = false
      player.currentTime = 0
      player.duration = 0
      player.queue = []
    }
    await loadPlaylists()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    deleting.value = false
  }
}

// ===== 播放（委托给全局播放器，切换页面不中断） =====
import { player, playSong as playerPlay } from '../utils/player'

function formatSize(bytes: number): string {
  if (!bytes) return ''
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}

/** 视频文件：WebView 的 <audio> 直接解码音轨，只出声不放画面 */
function isVideo(filename: string): boolean {
  return /\.(mp4|m4v|mov|webm|ogv|mkv)$/i.test(filename)
}

/** 从当前歌单发起播放（整个歌单作为队列，供全局播放条连播） */
function playSong(song: MusicSong) {
  if (!currentPlaylist.value) return
  playerPlay(song, currentPlaylist.value.name, currentPlaylist.value.songs)
}

onMounted(async () => {
  await loadPlaylists()
})

defineExpose({ loadPlaylists })
</script>

<template>
  <div class="music-page">
    <div class="music-toolbar">
      <span class="music-toolbar-title">{{ t('music.title') }}</span>
      <div class="music-actions">
        <button class="music-btn" :disabled="importing" @click="importFiles">
          {{ t('music.importFiles') }}
        </button>
        <button class="music-btn" :disabled="importing" @click="importFolder">
          {{ t('music.importFolder') }}
        </button>
      </div>
    </div>

    <div class="music-body">
      <!-- 歌单侧栏 -->
      <aside class="music-playlists">
        <div class="music-playlists-head">{{ t('music.playlists') }}</div>
        <div v-if="loading" class="music-empty">{{ t('music.loading') }}</div>
        <div v-else-if="playlists.length === 0" class="music-empty">
          {{ t('music.noPlaylists') }}
        </div>
        <ul v-else class="music-playlist-list">
          <li
            v-for="p in playlists"
            :key="p.name"
            class="music-playlist-item"
            :class="{ active: p.name === currentPlaylistName }"
            @click="currentPlaylistName = p.name"
          >
            <span class="music-playlist-name">{{ p.name }}</span>
            <span class="music-playlist-count">{{ p.songs.length }}</span>
            <button
              class="music-del-icon"
              :title="t('music.deletePlaylist')"
              :disabled="deleting"
              @click.stop="onDeletePlaylist(p.name)"
            >X</button>
          </li>
        </ul>
        <p v-if="importMsg" class="music-import-msg">{{ importMsg }}</p>
      </aside>

      <!-- 歌曲列表 -->
      <div class="music-songs">
        <div v-if="!currentPlaylist" class="music-empty music-empty-center">
          {{ t('music.selectPlaylist') }}
        </div>
        <template v-else>
          <div class="music-songs-head">
            <span>{{ currentPlaylist.name }}</span>
            <span class="music-songs-count">{{ currentPlaylist.songs.length }} {{ t('music.songs') }}</span>
          </div>
          <div v-if="currentPlaylist.songs.length === 0" class="music-empty music-empty-center">
            {{ t('music.noSongs') }}
          </div>
          <ul v-else class="music-song-list">
            <li
              v-for="song in currentPlaylist.songs"
              :key="song.filename"
              class="music-song-item"
              :class="{ active: player.current?.song.filename === song.filename }"
              @dblclick="playSong(song)"
            >
              <span class="music-song-name">{{ song.filename }}</span>
              <span v-if="isVideo(song.filename)" class="music-video-badge">{{ t('music.videoBadge') }}</span>
              <span class="music-song-size">{{ formatSize(song.size) }}</span>
              <button
                class="music-play-icon"
                :title="t('music.play')"
                @click.stop="playSong(song)"
              ><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg></button>
              <button
                class="music-del-icon"
                :title="t('music.removeSong')"
                :disabled="deleting"
                @click.stop="onRemoveSong(song)"
              ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button>
            </li>
          </ul>
        </template>
      </div>
    </div>

    <p v-if="error" class="music-error">{{ error }}</p>
  </div>
</template>

<style scoped>
.music-page {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--content-bg, transparent);
}

/* ===== 工具栏 ===== */
.music-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 14px 20px 10px 20px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.music-toolbar-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-color);
}

.music-actions {
  display: flex;
  gap: 8px;
}

.music-btn {
  padding: 6px 14px;
  border-radius: 8px;
  border: none;
  background: var(--button-bg);
  color: var(--text-color);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s ease;
}

.music-btn:hover:not(:disabled) {
  background: var(--button-hover-bg, var(--accent-color));
  color: #fff;
}

.music-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

/* ===== 主体布局 ===== */
.music-body {
  flex: 1;
  display: flex;
  min-height: 0;
}

.music-playlists {
  width: 210px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  padding: 10px 8px;
}

.music-playlists-head {
  font-size: 12px;
  color: var(--text-secondary, var(--text-color));
  opacity: 0.7;
  padding: 4px 8px 8px 8px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.music-playlist-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.music-playlist-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
  color: var(--text-color);
  font-size: 13px;
  transition: background 0.15s ease;
}

.music-playlist-item:hover {
  background: var(--button-bg);
}

.music-playlist-item.active {
  background: var(--accent-color);
  color: #fff;
}

.music-playlist-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.music-playlist-count {
  font-size: 11px;
  opacity: 0.6;
}

.music-playlist-item.active .music-playlist-count {
  opacity: 0.85;
}

.music-import-msg {
  margin: 10px 8px 0 8px;
  font-size: 12px;
  color: var(--text-secondary, var(--text-color));
  opacity: 0.8;
}

/* ===== 歌曲列表 ===== */
.music-songs {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow-y: auto;
  padding: 10px 16px;
}

.music-songs-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 4px 10px 4px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-color);
}

.music-songs-count {
  font-size: 12px;
  font-weight: 400;
  opacity: 0.65;
}

.music-song-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.music-song-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
  color: var(--text-color);
  font-size: 13px;
  transition: background 0.15s ease;
}

.music-song-item:hover {
  background: var(--button-bg);
}

.music-song-item.active {
  background: var(--accent-color);
  color: #fff;
}

.music-song-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.music-song-size {
  font-size: 11px;
  opacity: 0.6;
  flex-shrink: 0;
}

.music-video-badge {
  flex-shrink: 0;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 4px;
  background: rgba(128, 128, 128, 0.22);
  color: inherit;
  opacity: 0.75;
}

.music-play-icon,
.music-del-icon {
  flex-shrink: 0;
  border: none;
  background: transparent;
  color: inherit;
  opacity: 0.55;
  font-size: 12px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 4px;
}

.music-play-icon:hover,
.music-del-icon:hover:not(:disabled) {
  opacity: 1;
  background: rgba(128, 128, 128, 0.25);
}

.music-del-icon:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.music-play-icon svg,
.music-del-icon svg {
  width: 12px;
  height: 12px;
  display: block;
}

/* ===== 空态/错误 ===== */
.music-empty {
  font-size: 13px;
  color: var(--text-secondary, var(--text-color));
  opacity: 0.6;
  padding: 12px 8px;
}

.music-empty-center {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}

.music-error {
  margin: 0 20px 6px 20px;
  font-size: 12px;
  color: #e5484d;
}
</style>