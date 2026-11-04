plugins {
    // AGP 9 内置 Kotlin，这里只应用 android 应用插件
    id("com.android.application")
    // Compose 编译器（与 AGP 9 内置 Kotlin 2.2.10 匹配）
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.crystal.android"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.crystal.android"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        // 打包 jniLibs —— 把 rust 编译出的 .so 打进去
        ndk {
            // abiFilters 决定打进 APK 的架构
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }

    // AGP 9 内置 Kotlin：通过 KotlinAndroidProjectExtension 配置编译目标
    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }
}

dependencies {
    // uniffi 生成的 Kotlin 绑定依赖 JNA（Android 用 aar 即可，含架构 so）
    implementation("net.java.dev.jna:jna:5.6.0@aar")
    implementation("androidx.core:core-ktx:1.10.1")
    implementation("androidx.activity:activity-ktx:1.9.0")
    implementation("androidx.appcompat:appcompat:1.8.0")

    // Jetpack Compose（BOM 统一版本）
    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
}