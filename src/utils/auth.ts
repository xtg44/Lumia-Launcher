import { ref } from 'vue'
import { startDeviceAuth, pollDeviceAuth, getConfig, logout } from './tauri'
import { i18n } from '../i18n'

export function useAuth() {
  const isLoggedIn = ref(false)
  const msUsername = ref('')
  const msUuid = ref('')
  const authLoading = ref(false)
  const authError = ref('')
  const authStep = ref<'idle' | 'waiting_device_code' | 'polling' | 'complete' | 'error'>('idle')
  const deviceCode = ref('')
  const deviceCodeMessage = ref('')
  const verificationUri = ref('')

  let polling = false

  async function initAuth() {
    try {
      const config = await getConfig()
      if (config.ms_username && config.ms_uuid) {
        isLoggedIn.value = true
        msUsername.value = config.ms_username
        msUuid.value = config.ms_uuid
      }
    } catch {
      // 配置未加载，非关键
    }
  }

  async function startLogin() {
    authLoading.value = true
    authError.value = ''
    authStep.value = 'waiting_device_code'
    try {
      const result = await startDeviceAuth()
      if (result.user_code) {
        deviceCode.value = result.user_code
        deviceCodeMessage.value = result.message || i18n.global.t('home.browserCode')
        verificationUri.value = result.verification_uri || 'https://microsoft.com/devicelogin'
        authStep.value = 'polling'
        polling = true
        pollLoop()
      } else {
        authError.value = result.error || i18n.global.t('home.deviceCodeFailed')
        authStep.value = 'error'
      }
    } catch (e: any) {
      authError.value = e.toString()
      authStep.value = 'error'
    } finally {
      authLoading.value = false
    }
  }

  async function pollLoop() {
    while (polling && authStep.value === 'polling') {
      try {
        const result = await pollDeviceAuth()
        if (result.success) {
          isLoggedIn.value = true
          msUsername.value = result.username || ''
          msUuid.value = result.uuid || ''
          authStep.value = 'complete'
          deviceCodeMessage.value = result.message || i18n.global.t('home.loginSuccess')
          return
        } else if (result.message) {
          deviceCodeMessage.value = result.message
          await new Promise(resolve => setTimeout(resolve, 5000))
        } else if (result.error) {
          authError.value = result.error
          authStep.value = 'error'
          return
        }
      } catch (e: any) {
        authError.value = e.toString()
        authStep.value = 'error'
        return
      }
    }
  }

  async function handleLogout() {
    polling = false
    try {
      await logout()
    } catch {
      // 非关键
    }
    isLoggedIn.value = false
    msUsername.value = ''
    msUuid.value = ''
    authStep.value = 'idle'
    deviceCode.value = ''
    deviceCodeMessage.value = ''
    authError.value = ''
  }

  return {
    isLoggedIn,
    msUsername,
    msUuid,
    authLoading,
    authError,
    authStep,
    deviceCode,
    deviceCodeMessage,
    verificationUri,
    initAuth,
    startLogin,
    handleLogout,
  }
}
