// ============ 全局播放器（单例 store） ============
// 播放状态提升到应用级：切换页面时 <audio> 常驻不销毁，音乐继续放。
// PlayerBar.vue 负责渲染播放条 + audio 元素；MusicView.vue 通过这里的
// 函数发起播放。audio 元素绑定在模块级变量（组件挂载时 bindAudio）。

import { reactive } from 'vue'
import { musicStreamUrl } from './tauri'
import type { MusicSong } from './tauri'

export interface QueueItem {
  song: MusicSong
  playlist: string
}

export interface PlayerState {
  /** 当前播放的歌曲（null = 没有播放） */
  current: QueueItem | null
  playing: boolean
  currentTime: number
  duration: number
  volume: number
  /** 播放队列（当前歌单全部歌曲），供上一首/下一首/连播使用 */
  queue: QueueItem[]
}

export const player = reactive<PlayerState>({
  current: null,
  playing: false,
  currentTime: 0,
  duration: 0,
  volume: 0.8,
  queue: [],
})

/** 常驻的 <audio> 元素句柄（由 PlayerBar 绑定；切页不销毁） */
let audioEl: HTMLAudioElement | null = null

/** PlayerBar 挂载时调用；传入 null 解绑 */
export function bindAudio(el: HTMLAudioElement | null) {
  audioEl = el
  if (el) el.volume = player.volume
}

/** 从歌单发起播放：songs 为当前歌单全部歌曲，组成播放队列 */
export function playSong(song: MusicSong, playlist: string, songs: MusicSong[]) {
  player.queue = songs.map((s) => ({ song: s, playlist }))
  player.current = { song, playlist }
  player.currentTime = 0
  player.duration = 0
  if (!audioEl) return
  audioEl.src = musicStreamUrl(song.relPath)
  audioEl.volume = player.volume
  audioEl.play().catch(() => {
    player.playing = false
  })
  player.playing = true
}

/** 播放 / 暂停 */
export function togglePlay() {
  if (!audioEl || !player.current) return
  if (audioEl.paused) {
    audioEl.play().catch(() => {})
    player.playing = true
  } else {
    audioEl.pause()
    player.playing = false
  }
}

function move(step: number) {
  if (!player.current || player.queue.length === 0) return
  const idx = player.queue.findIndex(
    (q) => q.song.filename === player.current?.song.filename && q.playlist === player.current?.playlist,
  )
  if (idx < 0) return
  const next = (idx + step + player.queue.length) % player.queue.length
  const target = player.queue[next]
  playSong(target.song, target.playlist, player.queue.map((q) => q.song))
}

export function playNext() {
  move(1)
}

export function playPrev() {
  move(-1)
}

export function seek(time: number) {
  player.currentTime = time
  if (audioEl) audioEl.currentTime = time
}

export function setVolume(v: number) {
  player.volume = v
  if (audioEl) audioEl.volume = v
}

// ===== audio 元素事件回调（PlayerBar 的 <audio> 绑定到这些） =====
export function onTimeUpdate() {
  if (audioEl) player.currentTime = audioEl.currentTime
}

export function onLoadedMetadata() {
  if (audioEl) player.duration = audioEl.duration || 0
}

export function onEnded() {
  player.playing = false
  playNext()
}

export function onAudioError() {
  player.playing = false
}

export function formatTime(sec: number): string {
  if (!sec || Number.isNaN(sec)) return '0:00'
  const m = Math.floor(sec / 60)
  const s = Math.floor(sec % 60)
  return `${m}:${s.toString().padStart(2, '0')}`
}