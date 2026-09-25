import { openUrl } from './tauri'

const EASTER_EGG_URL = 'https://www.bilibili.com/video/BV1GJ411x7h7/'

export function triggerEasterEgg(): void {
  openUrl(EASTER_EGG_URL).catch(() => {
    window.open(EASTER_EGG_URL, '_blank')
  })
}