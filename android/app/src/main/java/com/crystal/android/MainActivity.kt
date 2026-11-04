package com.crystal.android

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.concurrent.thread
import uniffi.crystal_core.CoreException
import uniffi.crystal_core.VersionManifestItem
import uniffi.crystal_core.coreFetchVersions

// ========================= 桌面端配色（浅色 themeColor=#ffffff）精确还原 =========================
private val ColorText        = Color(0xFF1D1D1F)   // var(--text-color) light
private val ColorBorder      = Color(0x1A000000)   // var(--border-color) rgba(0,0,0,0.1)
private val ColorHeaderBg    = Color(0xB8FFFFFF)   // var(--header-bg) rgba(255,255,255,0.72)
private val ColorCard        = Color(0x1F787880)   // var(--card-bg) rgba(120,120,128,0.12)
private val ColorIndicator   = Color(0x29787880)   // var(--indicator-bg) rgba(120,120,128,0.16)
private val ColorAccent      = Color(0xFF007AFF)   // var(--accent-color) #007aff
private val ColorAccentText  = Color(0xFFFFFFFF)   // var(--accent-text-color) white
private val ColorAppBg       = Color(0xFFF5F5F7)   // app-container 浅色背景
private val ColorSideBorder  = Color(0xFFB3B3B3)   // --sidebar-border light
private val ColorLabel       = Color(0xFF6E6E73)   // --label-color light

// 侧边栏精确尺寸（对齐桌面 .nav-item 50×58、间隔4、indicator 50×50 r10）
private val NavItemW = 50.dp
private val NavItemH = 58.dp
private val NavIconS = 22.dp
private val IndicatorS = 50.dp

/** 与桌面 navItems 一致：home/version/download/terracotta/music/ai/plugin-center/settings */
private data class NavItem(val label: String, val iconRes: Int)
private val navItems = listOf(
    NavItem("首页", R.drawable.ic_nav_home),
    NavItem("版本", R.drawable.ic_nav_version),
    NavItem("下载", R.drawable.ic_nav_download),
    NavItem("联机", R.drawable.ic_nav_terracotta),
    NavItem("音乐", R.drawable.ic_nav_music),
    NavItem("AI",   R.drawable.ic_nav_ai),
    NavItem("插件", R.drawable.ic_nav_plugin),
    NavItem("设置", R.drawable.ic_nav_settings),
)

class MainActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { CrystalRoot() }
    }
}

@Composable
private fun CrystalRoot() {
    var selected by remember { mutableIntStateOf(0) }

    Column(modifier = Modifier.fillMaxSize().background(ColorAppBg)) {
        // ---- Header（对齐桌面 .header） ----
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(46.dp)
                .background(ColorHeaderBg)
                .border(1.dp, ColorBorder),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Spacer(Modifier.width(30.dp))
            Image(
                painter = painterResource(R.drawable.crystal_logo),
                contentDescription = "Crystal",
                modifier = Modifier.size(36.dp).clip(RoundedCornerShape(8.dp)),
            )
            Spacer(Modifier.width(8.dp))
            Text("Crystal", color = ColorText, fontSize = 32.sp, fontWeight = FontWeight.Medium)
        }

        // ---- main-content：侧边栏 + 内容 ----
        Row(modifier = Modifier.fillMaxSize()) {
            CrystalSidebar(selected) { selected = it }
            CrystalContent(selected)
        }
    }
}

// ---- 侧边栏（对齐桌面 .sidebar） ----
@Composable
private fun CrystalSidebar(selected: Int, onSelect: (Int) -> Unit) {
    // 桌面 indicator 步长 = 58(item)+4(margin) = 62；起始偏移 12px
    val step = NavItemH + 4.dp
    val indicatorY by animateDpAsState(
        targetValue = 12.dp + step * selected,
        animationSpec = tween(durationMillis = 320),
        label = "nav",
    )

    Box(
        modifier = Modifier
            .width(60.dp)
            .fillMaxHeight()
            .border(1.dp, ColorSideBorder),
    ) {
        // nav-indicator：50×50、圆角10、左距5
        Box(
            modifier = Modifier
                .offset(y = indicatorY)
                .padding(start = 5.dp)
                .size(IndicatorS)
                .clip(RoundedCornerShape(10.dp))
                .background(ColorIndicator),
        )

        // 桌面 nav-menu padding:2px 0 0；::before=6px → 首个 item 距顶 8px
        // 桌面 nav-item margin-bottom:4px → 项间距4dp、步长62dp
        Column(
            modifier = Modifier.fillMaxSize().padding(top = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            navItems.forEachIndexed { i, item ->
                Box(
                    modifier = Modifier.width(NavItemW).height(NavItemH),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                        modifier = Modifier.noRippleClick { onSelect(i) },
                    ) {
                        Icon(
                            painter = painterResource(item.iconRes),
                            contentDescription = item.label,
                            tint = ColorText,
                            modifier = Modifier.size(NavIconS),
                        )
                        Spacer(Modifier.height(3.dp))
                        Text(
                            item.label,
                            color = ColorText,
                            fontSize = 11.sp,
                            lineHeight = 13.sp,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Icon(painter: androidx.compose.ui.graphics.painter.Painter, contentDescription: String?, tint: Color, modifier: Modifier = Modifier) {
    Image(
        painter = painter,
        contentDescription = contentDescription,
        colorFilter = ColorFilter.tint(tint),
        modifier = modifier,
    )
}

@Composable
private fun Modifier.noRippleClick(onClick: () -> Unit): Modifier =
    this.then(Modifier.clickable(
        interactionSource = remember { MutableInteractionSource() },
        indication = null,
        onClick = onClick,
    ))

// ---- 内容区 ----
@Composable
private fun CrystalContent(index: Int) {
    Box(
        modifier = Modifier.fillMaxSize().padding(20.dp),
        contentAlignment = Alignment.TopCenter,
    ) {
        when (index) {
            0 -> HomePlaceholder()
            1 -> VersionsView()
            7 -> SettingsView()
            else -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Spacer(Modifier.height(40.dp))
                Text("即将推出", color = ColorLabel, fontSize = 14.sp)
            }
        }
    }
}

@Composable
private fun VersionsView() {
    // 桥接层是同步 FFI，放在后台线程调用，避免卡主线程
    var versions by remember { mutableStateOf(cachedVersions) }
    var error by remember { mutableStateOf(cachedError) }
    LaunchedEffect(Unit) {
        if (versions == null && error == null) {
            thread {
                try {
                    val list = coreFetchVersions()
                    cachedVersions = list
                    versions = list
                } catch (e: CoreException) {
                    val msg = (e as? CoreException.Message)?.detail ?: e.toString()
                    cachedError = msg
                    error = msg
                }
            }
        }
    }
    Box(
        modifier = Modifier.fillMaxWidth(),
        contentAlignment = Alignment.Center,
    ) {
        when {
            error != null -> Text("加载失败：$error", color = ColorLabel, fontSize = 14.sp)
            versions == null -> Text("加载中…", color = ColorLabel, fontSize = 14.sp)
            versions!!.isEmpty() -> Text("暂无版本", color = ColorLabel, fontSize = 14.sp)
            else -> LazyColumn {
                items(versions!!) { v ->
                    Row(
                        modifier = Modifier.fillMaxWidth()
                            .padding(vertical = 10.dp)
                            .border(1.dp, ColorCard, RoundedCornerShape(8.dp))
                            .padding(horizontal = 14.dp, vertical = 12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(v.id, color = ColorText, fontSize = 15.sp)
                        Text(v.category, color = ColorLabel, fontSize = 12.sp)
                    }
                }
            }
        }
    }
}

// 简单状态缓存，保证重组合不重复触发后台拉取
private var cachedVersions: List<VersionManifestItem>? = null
private var cachedError: String? = null

@Composable
private fun HomePlaceholder() {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(20.dp))
        Text("你好，玩家", color = ColorText, fontSize = 36.sp, fontWeight = FontWeight.Light)
        Spacer(Modifier.height(20.dp))

        // 在线 / 离线按钮
        Row(horizontalArrangement = Arrangement.spacedBy(17.dp)) {
            Box(
                modifier = Modifier.height(28.dp).clip(RoundedCornerShape(10.dp))
                    .background(ColorAccent).padding(horizontal = 12.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("在线", color = ColorAccentText, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            }
            Box(
                modifier = Modifier.height(28.dp).clip(RoundedCornerShape(10.dp))
                    .background(ColorCard).padding(horizontal = 12.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("离线", color = ColorText, fontSize = 13.sp)
            }
        }

        Spacer(Modifier.height(30.dp))
        Box(
            modifier = Modifier.size(90.dp).clip(RoundedCornerShape(12.dp)).background(ColorCard),
            contentAlignment = Alignment.Center,
        ) { Text("头像", color = ColorText, fontSize = 14.sp) }

        Spacer(Modifier.height(20.dp))
        Text("Steve", color = ColorText, fontSize = 36.sp, fontWeight = FontWeight.Light)

        Spacer(Modifier.height(30.dp))
        Column(
            modifier = Modifier
                .clip(RoundedCornerShape(25.dp))
                .background(ColorCard)
                .padding(horizontal = 59.dp, vertical = 5.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("开始游戏", color = ColorText, fontSize = 32.sp, fontWeight = FontWeight.Normal)
            Spacer(Modifier.height(7.dp))
            Text("Minecraft 1.21", color = ColorText, fontSize = 16.sp, lineHeight = 19.sp)
        }
    }
}

// ====================================================================
// 设置界面
// ====================================================================
private val themeColors = listOf(
    "#ffffff", "#e94560", "#ff6b6b", "#feca57", "#48dbfb",
    "#1dd1a1", "#5f27cd", "#ff9ff3", "#54a0ff", "#00d2d3", "#ff9f43"
)

@Composable
private fun SettingsView() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val prefs = remember { context.getSharedPreferences("crystal_settings", 0) }

    var username by remember { mutableStateOf(prefs.getString("username", "") ?: "") }
    var themeKey by remember { mutableStateOf(prefs.getString("theme", "#ffffff") ?: "#ffffff") }
    var darkMode by remember { mutableStateOf(prefs.getBoolean("dark", false)) }
    var language by remember { mutableStateOf(prefs.getString("language", "auto") ?: "auto") }
    var autoUpdate by remember { mutableStateOf(prefs.getBoolean("auto_update", true)) }

    fun save(k: String, v: Any) {
        prefs.edit().apply {
            when (v) {
                is String -> putString(k, v)
                is Boolean -> putBoolean(k, v)
            }
        }.apply()
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize().fillMaxWidth(),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(bottom = 24.dp),
    ) {
        item { SectionTitle("外观") }
        item {
            SettingCard {
                Text("主题颜色", color = ColorText, fontSize = 15.sp)
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    themeColors.forEach { c ->
                        val color = android.graphics.Color.parseColor(c)
                        val isActive = c == themeKey
                        Box(
                            modifier = Modifier
                                .size(34.dp)
                                .clickable {
                                    themeKey = c
                                    save("theme", c)
                                }
                                .border(if (isActive) 3.dp else 1.dp,
                                    if (isActive) ColorAccent else ColorBorder,
                                    RoundedCornerShape(50))
                                .padding(4.dp)
                                .clip(RoundedCornerShape(50))
                                .background(androidx.compose.ui.graphics.Color(color)),
                            contentAlignment = Alignment.Center,
                        ) {
                            if (isActive) Text("✓", color = androidx.compose.ui.graphics.Color.White, fontSize = 13.sp)
                        }
                    }
                }
            }
        }
        item {
            SettingCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("深色模式", color = ColorText, fontSize = 15.sp, modifier = Modifier.weight(1f))
                    Toggle(darkMode) {
                        darkMode = it
                        save("dark", it)
                    }
                }
            }
        }

        item { SectionTitle("游戏") }
        item {
            SettingCard {
                Text("用户名", color = ColorText, fontSize = 15.sp)
                OutlinedField(username, { username = it }, onDone = { save("username", username) })
            }
        }
        item {
            SettingCard {
                Text("游戏目录", color = ColorText, fontSize = 15.sp)
                Text("（仅桌面端）", color = ColorLabel, fontSize = 13.sp)
            }
        }

        item { SectionTitle("其他") }
        item {
            SettingCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("语言", color = ColorText, fontSize = 15.sp, modifier = Modifier.weight(1f))
                    Text(language.takeIf { it != "auto" } ?: "跟随系统", color = ColorLabel, fontSize = 13.sp)
                }
            }
        }
        item {
            SettingCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("自动更新", color = ColorText, fontSize = 15.sp, modifier = Modifier.weight(1f))
                    Toggle(autoUpdate) {
                        autoUpdate = it
                        save("auto_update", it)
                    }
                }
            }
        }

        item { SectionTitle("高级") }
        item { SettingCard { Text("Java 参数", color = ColorText, fontSize = 15.sp); Text("（仅桌面端）", color = ColorLabel, fontSize = 13.sp) } }
        item { SectionTitle("人工智能") }
        item { SettingCard { Text("AI 助手配置", color = ColorText, fontSize = 15.sp); Text("（仅桌面端）", color = ColorLabel, fontSize = 13.sp) } }
        item { SectionTitle("工具箱") }
        item { SettingCard { Text("文件下载器", color = ColorText, fontSize = 15.sp); Text("（仅桌面端）", color = ColorLabel, fontSize = 13.sp) } }
        item { SectionTitle("侧边栏") }
        item { SettingCard { Text("导航项显隐", color = ColorText, fontSize = 15.sp); Text("（仅桌面端）", color = ColorLabel, fontSize = 13.sp) } }

        item { SectionTitle("关于") }
        item {
            SettingCard {
                AboutRow("版本", "0.1.0")
                AboutRow("作者", "小甜瓜")
                AboutRow("许可", "MIT License")
            }
        }
    }
}

@Composable
private fun SectionTitle(title: String) {
    Text(title, color = ColorLabel, fontSize = 13.sp, fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(top = 18.dp, bottom = 8.dp, start = 4.dp))
}

@Composable
private fun SettingCard(content: @Composable () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 10.dp)
            .border(1.dp, ColorCard, RoundedCornerShape(8.dp))
            .background(ColorHeaderBg)
            .padding(horizontal = 14.dp, vertical = 12.dp),
    ) { content() }
}

@Composable
private fun AboutRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = ColorLabel, fontSize = 14.sp)
        Text(value, color = ColorText, fontSize = 14.sp)
    }
}

@Composable
private fun Toggle(checked: Boolean, onChange: (Boolean) -> Unit) {
    val h = 22.dp
    val w = 40.dp
    Box(
        modifier = Modifier
            .width(w).height(h)
            .clickable { onChange(!checked) }
            .clip(RoundedCornerShape(50))
            .background(if (checked) ColorAccent else ColorCard),
        contentAlignment = if (checked) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Box(
            modifier = Modifier.padding(3.dp).size(h - 6.dp).clip(RoundedCornerShape(50)).background(Color.White),
        )
    }
}

@Composable
private fun OutlinedField(value: String, onChange: (String) -> Unit, onDone: () -> Unit) {
    androidx.compose.foundation.text.BasicTextField(
        value = value,
        onValueChange = onChange,
        textStyle = androidx.compose.ui.text.TextStyle(color = ColorText, fontSize = 15.sp),
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 6.dp)
            .background(androidx.compose.ui.graphics.Color(0x14000000), RoundedCornerShape(6.dp))
            .padding(horizontal = 10.dp, vertical = 8.dp),
        singleLine = true,
        decorationBox = { inner -> inner() },
    )
    Spacer(Modifier.height(6.dp))
}