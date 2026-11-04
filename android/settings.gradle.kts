// 仓库源：Google 官方 + 腾讯/阿里镜像做 fallback
pluginManagement {
    repositories {
        google()
        maven { url = uri("https://mirrors.cloud.tencent.com/nexus/repository/maven-public/") }
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        maven { url = uri("https://mirrors.cloud.tencent.com/nexus/repository/maven-public/") }
        mavenCentral()
    }
}

rootProject.name = "lumia-android"
include(":app")