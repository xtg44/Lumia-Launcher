//! macOS Touch Bar 支持（仅 macOS 编译）
//!
//! 在窗口底部 Touch Bar 上放置：
//!   启动 / 主页 / 版本 / 下载 / 联机 / AI / 设置 / 音乐 / 下载进度条
//!
//! 点击按钮 → 通过 Tauri 事件 `touchbar-action` 发送动作名（如 "launch" / "home"）给前端；
//! 前端更新进度 → 调用 `touchbar_set_progress` 命令刷新进度条。
//!
//! # 线程说明
//! AppKit 控件只能在主线程操作。所有 objc2 调用都发生在：
//! - setup（Tauri setup 闭包在主线程执行）
//! - NSTouchBarDelegate 回调（AppKit 主线程）
//! - `set_progress` 通过 `app.run_on_main_thread` 派发到主线程
//! 静态变量（KeepAlive / PROGRESS_PTR）仅在主线程访问，因此指针安全。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSCustomTouchBarItem, NSColor, NSProgressIndicator, NSProgressIndicatorStyle,
    NSTouchBar, NSTouchBarDelegate, NSTouchBarItem, NSTouchBarItemIdentifier, NSWindow,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSInteger, NSObject, NSObjectProtocol, NSString,
};
use tauri::{AppHandle, Emitter, Manager};

/// Touch Bar 按钮定义（id 与按钮 tag 一一对应）
struct ButtonDef {
    id: Retained<NSString>,
    title: Retained<NSString>,
    /// 点击后通过 touchbar-action 事件发送给前端的动作名
    action: &'static str,
    /// 是否使用强调色（启动按钮用绿色）
    green: bool,
}

/// LumiaTouchBarDelegate 的实例变量
struct LumiaTouchBarIvars {
    buttons: Vec<ButtonDef>,
    progress_id: Retained<NSString>,
    progress: RefCell<Option<Retained<NSProgressIndicator>>>,
    on_action: Rc<dyn Fn(&str)>,
}

// ===== 自定义 AppKit 类：Touch Bar 代理（同时充当按钮 target） =====
define_class!(
    /// 实现 NSTouchBarDelegate：按 identifier 创建按钮 / 进度条；
    /// 额外实现 btnClicked: 处理按钮点击（tag → 动作名 → 发送事件）。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = LumiaTouchBarIvars]
    struct LumiaTouchBarDelegate;

    unsafe impl NSObjectProtocol for LumiaTouchBarDelegate {}

    unsafe impl NSTouchBarDelegate for LumiaTouchBarDelegate {
        #[unsafe(method_id(touchBar:makeItemForIdentifier:))]
        #[allow(non_snake_case)]
        fn touchBar_makeItemForIdentifier(
            &self,
            _touch_bar: &NSTouchBar,
            identifier: &NSTouchBarItemIdentifier,
        ) -> Option<Retained<NSTouchBarItem>> {
            // method_id 包装下不能用早期 return，实际逻辑放在宏外的 make_item
            self.make_item(identifier)
        }
    }

    impl LumiaTouchBarDelegate {
        /// 按钮点击回调（target 是本代理，action 选择器 btnClicked:）
        #[unsafe(method(btnClicked:))]
        fn btn_clicked(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let tag: NSInteger = unsafe { msg_send![sender, tag] };
            if let Some(def) = self.ivars().buttons.get(tag as usize) {
                (self.ivars().on_action)(def.action);
            }
        }
    }
);

// 宏外辅助方法（可以自由使用早期 return）
impl LumiaTouchBarDelegate {
    fn make_item(
        &self,
        identifier: &NSTouchBarItemIdentifier,
    ) -> Option<Retained<NSTouchBarItem>> {
        let Some(mtm) = MainThreadMarker::new() else {
            return None; // 非主线程，不应发生（AppKit 回调在主线程）
        };
        let ivars = self.ivars();

        // 进度条 item
        if identifier.isEqualToString(&ivars.progress_id) {
            let indicator = NSProgressIndicator::new(mtm);
            indicator.setStyle(NSProgressIndicatorStyle::Bar);
            indicator.setMinValue(0.0);
            indicator.setMaxValue(100.0);
            indicator.setDoubleValue(0.0);
            let item = NSCustomTouchBarItem::initWithIdentifier(
                NSCustomTouchBarItem::alloc(mtm),
                identifier,
            );
            item.setView(&indicator);
            *ivars.progress.borrow_mut() = Some(indicator.clone());
            let ptr = &*indicator as *const NSProgressIndicator as *mut _;
            *PROGRESS_PTR.lock().expect("touchbar pointer lock") = Some(ProgressPtr(ptr));
            return Some(unsafe { Retained::cast_unchecked(item) });
        }

        // 按钮 item
        for (idx, def) in ivars.buttons.iter().enumerate() {
            if identifier.isEqualToString(&def.id) {
                let button = NSButton::new(mtm);
                unsafe {
                    let () = msg_send![&*button, setTitle: &*def.title];
                    let () = msg_send![&*button, setTag: idx as NSInteger];
                    let () = msg_send![&*button, setTarget: &*self];
                    let () =
                        msg_send![&*button, setAction: Sel::register(c"btnClicked:")];
                }
                if def.green {
                    unsafe {
                        let () = msg_send![
                            &*button,
                            setContentTintColor: &*NSColor::systemGreenColor()
                        ];
                    }
                }
                let item = NSCustomTouchBarItem::initWithIdentifier(
                    NSCustomTouchBarItem::alloc(mtm),
                    identifier,
                );
                item.setView(&button);
                return Some(unsafe { Retained::cast_unchecked(item) });
            }
        }
        None
    }
}

// ===== 生命周期保持（对象只在主线程访问） =====
// 字段只用于 keep-alive（持有引用防止释放），本意就是不被读取
#[allow(dead_code)]
struct KeepAlive {
    delegate: Retained<LumiaTouchBarDelegate>,
    touch_bar: Retained<NSTouchBar>,
}
// SAFETY: 两个字段只在主线程使用（见模块头注释）
unsafe impl Send for KeepAlive {}

static KEEP: Mutex<Option<KeepAlive>> = Mutex::new(None);

/// 进度条指针包装（objc2 的 MainThreadOnly autotrait 会让裸指针不满足 Send；
/// 指针只在主线程访问，见模块头注释，因此手动放宽）
#[derive(Clone, Copy)]
struct ProgressPtr(*mut NSProgressIndicator);
// SAFETY: 仅主线程访问（见模块头注释）
unsafe impl Send for ProgressPtr {}

impl ProgressPtr {
    /// 更新进度值（unsafe objc2 调用；必须已在主线程）
    fn set_double_value(&self, value: f64) {
        unsafe {
            (&*self.0).setDoubleValue(value);
        }
    }
}

static PROGRESS_PTR: Mutex<Option<ProgressPtr>> = Mutex::new(None);

/// 按钮定义：id / 标题 / 动作名 / 强调色（顺序即 Touch Bar 排列与 tag）
const BUTTON_DEFS: [(&str, &str, &str, bool); 8] = [
    ("com.lumia.touchbar.launch", "启动", "launch", true),
    ("com.lumia.touchbar.home", "主页", "home", false),
    ("com.lumia.touchbar.versions", "版本", "versions", false),
    ("com.lumia.touchbar.download", "下载", "download", false),
    ("com.lumia.touchbar.terracotta", "联机", "terracotta", false),
    ("com.lumia.touchbar.ai", "AI", "ai", false),
    ("com.lumia.touchbar.settings", "设置", "settings", false),
    ("com.lumia.touchbar.music", "音乐", "music", false),
];

const FLEX: &str = "NSTouchBarItemIdentifierFlexibleSpace";

/// 初始化 Touch Bar（必须在主线程调用，Tauri setup 闭包满足）
pub fn setup(app: &AppHandle) -> Result<(), String> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Ok(()); // 不在主线程，跳过（一般不会发生）
    };

    // 点击回调：通过 Tauri 事件把动作名发给前端
    let app_clone = app.clone();
    let on_action: Rc<dyn Fn(&str)> = Rc::new(move |action: &str| {
        let _ = app_clone.emit("touchbar-action", action.to_string());
    });
    // 说明：emit 是异步的，跨进程事件会在主线程事件循环中派发，这里无需等待

    let buttons: Vec<ButtonDef> = BUTTON_DEFS
        .iter()
        .map(|(id, title, action, green)| ButtonDef {
            id: NSString::from_str(id),
            title: NSString::from_str(title),
            action,
            green: *green,
        })
        .collect();

    let delegate: Retained<LumiaTouchBarDelegate> = {
        let this = LumiaTouchBarDelegate::alloc(mtm).set_ivars(LumiaTouchBarIvars {
            buttons,
            progress_id: NSString::from_str("com.lumia.touchbar.progress"),
            progress: RefCell::new(None),
            on_action,
        });
        unsafe { msg_send![super(this), init] }
    };

    let touch_bar = NSTouchBar::new(mtm);
    touch_bar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    // 排列：启动 | 主页 版本 下载 | 联机 AI 设置 音乐 | 进度条
    let ids: Vec<Retained<NSString>> = [
        BUTTON_DEFS[0].0,
        FLEX,
        BUTTON_DEFS[1].0,
        BUTTON_DEFS[2].0,
        BUTTON_DEFS[3].0,
        FLEX,
        BUTTON_DEFS[4].0,
        BUTTON_DEFS[5].0,
        BUTTON_DEFS[6].0,
        BUTTON_DEFS[7].0,
        FLEX,
        "com.lumia.touchbar.progress",
    ]
    .iter()
    .map(|s| NSString::from_str(s))
    .collect();
    let ids_array = NSArray::from_retained_slice(&ids);
    touch_bar.setDefaultItemIdentifiers(&ids_array);

    // 挂到主窗口
    if let Some(webview) = app.get_webview_window("main") {
        if let Ok(ns_window) = webview.ns_window() {
            unsafe {
                let () = msg_send![ns_window as *mut NSWindow, setTouchBar: &*touch_bar];
            }
        }
    }

    // 保持 delegate 与 touch bar 存活（delegate 属性是弱引用，不 keep 会被释放）
    *KEEP.lock().expect("touchbar keep lock") = Some(KeepAlive { delegate, touch_bar });
    Ok(())
}

/// 更新 Touch Bar 进度条（0-100，任意线程调用，内部派发到主线程）
pub fn set_progress(app: &AppHandle, progress: f64) {
    let p = match PROGRESS_PTR.lock().expect("touchbar pointer lock").as_ref() {
        Some(p) => *p,
        None => return,
    };
    let _ = app.run_on_main_thread(move || {
        p.set_double_value(progress);
    });
}

/// 根据侧边栏可见性更新 Touch Bar 按钮
pub fn update_active_items(_app: &AppHandle, active: &[String]) {
    let keep = KEEP.lock().expect("touchbar keep lock");
    let Some(ref alive) = *keep else { return };
    if MainThreadMarker::new().is_none() {
        return;
    }

    let action_to_id: Vec<(&str, &str)> = BUTTON_DEFS
        .iter()
        .map(|(id, _title, action, _green)| (*action, *id))
        .collect();

    let active_set: std::collections::HashSet<&str> = active.iter().map(|s| s.as_str()).collect();

    let mut ids: Vec<Retained<NSString>> = Vec::new();
    ids.push(NSString::from_str(BUTTON_DEFS[0].0)); // 启动
    ids.push(NSString::from_str(FLEX));
    for (action, id) in &action_to_id {
        if *action == "launch" {
            continue;
        }
        if active_set.contains(action) {
            ids.push(NSString::from_str(id));
        }
    }
    ids.push(NSString::from_str(FLEX));
    ids.push(NSString::from_str("com.lumia.touchbar.progress"));

    let ids_array = NSArray::from_retained_slice(&ids);
    unsafe {
        let () = msg_send![&*alive.touch_bar, setDefaultItemIdentifiers: &*ids_array];
    }
}