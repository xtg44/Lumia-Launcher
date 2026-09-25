import { createApp } from "vue";
import App from "./App.vue";
import "./assets/main.css";
import { i18n, resolveLocale, applyLocale } from "./i18n";
import { getConfig } from "./utils/tauri";

// 启动时解析语言：设置里的偏好（auto/zh-CN/en/ja/ko）→ auto 时跟随系统语言
// （zh 开头用中文、ja 开头用日语、ko 开头用韩语，其余一律英文）。
// 系统语言用 navigator.language（Tauri WebView 反映操作系统语言）。
// 在挂载前完成，避免界面先闪中文再切英文。
async function bootstrap() {
  let pref = "auto";
  try {
    const config = await getConfig();
    pref = (config.language as string) || "auto";
  } catch {
    // 配置读取失败时保持 auto
  }
  const systemLocale = navigator.language || navigator.languages?.[0] || "en";
  applyLocale(resolveLocale(pref, systemLocale));
  createApp(App).use(i18n).mount("#app");
}

bootstrap();
