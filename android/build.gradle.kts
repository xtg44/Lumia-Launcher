plugins {
    // AGP 9 起内置 Kotlin 支持，无需再单独声明 org.jetbrains.kotlin.android
    id("com.android.application") version "9.3.0" apply false
    // Compose 编译器插件，版本必须与 AGP 9.x 内置 Kotlin 2.2.10 匹配
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10" apply false
}