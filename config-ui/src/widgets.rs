//! 最小组件集：开关、数值滑块、字体下拉。均为纯视图构建器；事件由父级 Entity 挂监听并统一计脏。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use gpui::{Bounds, MouseDownEvent, Pixels, div, deferred, prelude::*, px, relative};

use crate::theme;

#[cfg(windows)]
use fontdb::{Database, Source};

fn rgb_(v: u32) -> gpui::Rgba {
    gpui::rgb(v)
}

// 获取系统字体列表（Windows 使用 fontdb 扫描系统字体目录）
fn system_font_families() -> &'static Vec<String> {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(|| {
        #[cfg(windows)]
        {
            let mut db = Database::new();
            db.load_system_fonts();
            let mut families: Vec<String> = db.faces()
                .flat_map(|f| f.families.iter().map(|(name, _lang)| name.clone()))
                .collect();
            families.sort();
            families.dedup();
            families
        }
        #[cfg(not(windows))]
        {
            vec!["JetBrains Mono".to_string(), "Consolas".to_string(), "Monospace".to_string()]
        }
    })
}

// ---------- 开关 ----------

pub fn toggle(id: &'static str, value: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .bg(if value {
            theme::accent_dim()
        } else {
            rgb_(0x39424e)
        })
        .p(px(2.))
        .flex()
        .when(value, |d| d.justify_end())
        .when(!value, |d| d.justify_start())
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .child(div().size(px(16.)).rounded_full().bg(if value {
            theme::accent()
        } else {
            theme::fg_dim()
        }))
}

// ---------- 数值滑块 ----------

pub type SharedBounds = Rc<RefCell<Option<Bounds<Pixels>>>>;

/// 水平滑块。bounds_cell 由父级持有，paint 时回写轨道几何，
/// 父级鼠标事件据此把 x 换算成值。返回元素供父级继续挂鼠标监听。
pub fn slider(
    id: &'static str,
    value: f32,
    min: f32,
    max: f32,
    bounds_cell: SharedBounds,
) -> gpui::Stateful<gpui::Div> {
    let frac = ((value - min) / (max - min).max(1e-6)).clamp(0., 1.);
    div()
        .id(id)
        .w_full()
        .h(px(20.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .relative()
                .flex_1()
                .h(px(4.))
                .rounded_full()
                .bg(rgb_(0x39424e))
                .child(canvas_store_bounds(bounds_cell.clone()))
                .child(
                    div()
                        .absolute()
                        .top(px(-6.))
                        .left(relative(frac))
                        .size(px(16.))
                        .rounded_full()
                        .bg(theme::accent())
                        .border_2()
                        .border_color(theme::bg_base()),
                ),
        )
}

/// 不可见元素：prepaint 时把自身 bounds 写进共享槽位。
fn canvas_store_bounds(slot: SharedBounds) -> gpui::Canvas<()> {
    gpui::canvas(
        move |bounds, _, _| {
            *slot.borrow_mut() = Some(bounds);
        },
        |_, _, _, _| {},
    )
}

// ---------- 分段选择器 ----------

/// 分段选择器：一行选项，选中的高亮。selected 回调由父级对每个选项 div 挂 on_mouse_down 实现，
/// 本函数只管外观。
pub fn segment(
    id: impl Into<gpui::SharedString>,
    label: &str,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py_1()
        .rounded_sm()
        .text_size(px(12.))
        .cursor_pointer()
        .text_color(if selected { theme::bg_base() } else { theme::fg_main() })
        .bg(if selected { theme::accent() } else { theme::bg_elevated() })
        .border_1()
        .border_color(theme::border())
        .child(label.to_string())
}

/// 把窗口坐标换算为滑块值（父级事件处理器用）。
pub fn slider_value_at(slot: &SharedBounds, x: Pixels, min: f32, max: f32) -> f32 {
    let guard = slot.borrow();
    let Some(b) = guard.as_ref() else {
        return min;
    };
    let frac = ((x - b.left()) / b.size.width.max(px(1.))).clamp(0.0, 1.0);
    min + frac as f32 * (max - min)
}

// ---------- 常用等宽字体预设 ----------
/// 常用等宽字体预设，避免扫描全系统字体（启动慢、列表太长）。
/// 用户如需其他字体可在高级区手动填入。
pub const COMMON_MONO_FONTS: &[&str] = &[
    "JetBrains Mono",
    "Cascadia Code",
    "Consolas",
    "Fira Code",
    "Source Code Pro",
    "Ubuntu Mono",
    "Monospace",
];

// ---------- 字体下拉选择器 ----------
/// 折叠式字体下拉：默认只显示当前字体 + 三角箭头；展开后以浮层列出全部系统字体，
/// 滚动选择。selected: 当前选中的字体；open: 是否展开；
/// on_toggle: 点击触发收起/展开（由父级切换 open）；on_select: 某项被点击；
/// on_close: 点击下拉区域之外时收起（由父级把 open 置 false）。
pub fn font_dropdown<F, G, H>(
    selected_family: &str,
    open: bool,
    on_toggle: F,
    on_select: G,
    on_close: H,
) -> gpui::Stateful<gpui::Div>
where
    F: Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    G: Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
    H: Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
{
    let families = system_font_families();
    let on_select = Rc::new(on_select);

    // 折叠时的当前值按钮（同时是展开/收起的触发器）
    let current = div()
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_1()
        .bg(theme::bg_elevated())
        .border_1()
        .border_color(theme::border())
        .text_size(px(12.))
        .text_color(theme::fg_main())
        .child(selected_family.to_string())
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme::fg_dim())
                .child(if open { "▲" } else { "▼" }),
        )
        .on_mouse_down(gpui::MouseButton::Left, on_toggle);

    // 展开后的浮点列表：绝对定位在当前行下方，滚动显示全部字体
    let list = div()
        .id("font-dropdown-list")
        .absolute()
        .top(px(26.))
        .left(px(0.))
        .w(px(300.))
        .max_h(px(300.))
        .overflow_y_scroll()
        .rounded_sm()
        .bg(theme::bg_panel())
        .border_1()
        .border_color(theme::border())
        .text_size(px(12.))
        .children(families.iter().map(|fam| {
            let is_sel = fam == selected_family;
            let fam_str = fam.clone();
            let on_select = on_select.clone();
            div()
                .px_2()
                .py_1()
                .cursor_pointer()
                .text_color(if is_sel { theme::accent() } else { theme::fg_main() })
                .when(is_sel, |d| d.bg(theme::accent_dim()))
                .hover(|s| s.bg(theme::bg_elevated()))
                .child(fam_str.clone())
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    on_select(&fam_str, window, cx)
                })
        }));

    div()
        .id("font-selector")
        .relative()
        .child(current)
        .when(open, |d| d.child(deferred(list).with_priority(1)))
        .on_mouse_down_out(on_close)
}
