use crate::customglyph::*;
use crate::tabbar::{TabBarItem, TabEntry};
use crate::termwindow::box_model::*;
use crate::termwindow::render::corners::*;

use crate::termwindow::render::window_buttons::window_button_element;
use crate::termwindow::{UIItem, UIItemType};
use crate::utilsprites::RenderMetrics;
use config::{Dimension, DimensionContext, TabBarColors};
use std::rc::Rc;
use std::time::Duration;
use wezterm_font::LoadedFont;
use wezterm_term::color::{ColorAttribute, ColorPalette};
use window::{IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle};

/// 内置 iconfont 的字形码点（私有区，见 assets/fonts/iconfont.ttf）
pub mod iconfont_glyph {
    pub const SETTINGS: char = '\u{e607}';
    pub const COPY: char = '\u{e65f}';
    pub const SFTP: char = '\u{e713}';
    /// 清理缓存（U+E641）：先存档到 exe 同目录 screen-shot/ 再清屏
    pub const CLEAR: char = '\u{e641}';
    /// 发送指令（U+E6E4，cloudshell 云命令行）：点击弹出 tab_button_commands
    /// 指令选择列表
    pub const SEND: char = '\u{e6e4}';
}

/// 标签栏按钮（+/齿轮/复制/SFTP）的图标默认蓝色；
/// hover 时稍微提亮。复制成功反馈期图标转绿色。
/// SFTP 按钮在非 SSH 标签下灰色不可用。
/// sRGB 值，构造时需经 to_linear() 转换。
mod button_colors {
    use ::window::color::LinearRgba;

    pub const ICON_BLUE: [u8; 3] = [0x2f, 0x81, 0xf7];
    pub const ICON_BLUE_HOVER: [u8; 3] = [0x5a, 0x9d, 0xf9];
    pub const ICON_GREEN: [u8; 3] = [0x3f, 0xb9, 0x50];
    pub const ICON_DIM: [u8; 3] = [0x5d, 0x66, 0x73];

    fn linear(rgb: [u8; 3]) -> LinearRgba {
        LinearRgba::with_srgba(rgb[0], rgb[1], rgb[2], 0xff)
    }

    pub fn icon_blue() -> LinearRgba {
        linear(ICON_BLUE)
    }

    pub fn icon_blue_hover() -> LinearRgba {
        linear(ICON_BLUE_HOVER)
    }

    pub fn icon_green() -> LinearRgba {
        linear(ICON_GREEN)
    }

    pub fn icon_dim() -> LinearRgba {
        linear(ICON_DIM)
    }
}

const X_BUTTON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
];

const PLUS_BUTTON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
];

/// 设置/复制按钮改用内置 iconfont 字形（见 iconfont_glyph）渲染，
/// 由专用字体实例（句柄链仅含 iconfont）绘制，不受终端字体回退链影响。
impl crate::TermWindow {
    pub fn invalidate_fancy_tab_bar(&mut self) {
        self.fancy_tab_bar.take();
    }

    pub fn build_fancy_tab_bar(&self, palette: &ColorPalette) -> anyhow::Result<ComputedElement> {
        let tab_bar_height = self.tab_bar_pixel_height()?;
        let font = self.fonts.title_font()?;
        let icon_font = self.fonts.icon_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let items = self.tab_bar.items();
        let colors = self
            .config
            .colors
            .as_ref()
            .and_then(|c| c.tab_bar.as_ref())
            .cloned()
            .unwrap_or_else(TabBarColors::default);

        let mut left_status = vec![];
        let mut left_eles = vec![];
        let mut right_eles = vec![];
        // 新标签（+）按钮单独收集：有标签时紧随最后一个标签入左列，
        // 零标签空窗口时回归右侧按钮组首位（见下方分流注释）
        let mut new_tab_eles = vec![];
        let bar_colors = ElementColors {
            border: BorderColor::default(),
            bg: if self.focused.is_some() {
                self.config.window_frame.active_titlebar_bg
            } else {
                self.config.window_frame.inactive_titlebar_bg
            }
            .to_linear()
            .into(),
            text: if self.focused.is_some() {
                self.config.window_frame.active_titlebar_fg
            } else {
                self.config.window_frame.inactive_titlebar_fg
            }
            .to_linear()
            .into(),
        };

        let item_to_elem = |item: &TabEntry| -> Element {
            let element = Element::with_line(&font, &item.title, palette);

            let bg_color = item
                .title
                .get_cell(0)
                .and_then(|c| match c.attrs().background() {
                    ColorAttribute::Default => None,
                    col => Some(palette.resolve_bg(col)),
                });
            let fg_color = item
                .title
                .get_cell(0)
                .and_then(|c| match c.attrs().foreground() {
                    ColorAttribute::Default => None,
                    col => Some(palette.resolve_fg(col)),
                });

            let new_tab = colors.new_tab();
            let new_tab_hover = colors.new_tab_hover();
            let active_tab = colors.active_tab();

            match item.item {
                TabBarItem::RightStatus | TabBarItem::LeftStatus | TabBarItem::None => element
                    .item_type(UIItemType::TabBar(TabBarItem::None))
                    .line_height(Some(1.75))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.0),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.),
                        bottom: Dimension::Cells(0.),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(0.)))
                    .colors(bar_colors.clone()),
                TabBarItem::NewTabButton => Element::new(
                    &font,
                    ElementContent::Poly {
                        line_width: metrics.underline_height.max(2),
                        poly: SizedPoly {
                            poly: PLUS_BUTTON,
                            width: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
                            height: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
                        },
                    },
                )
                .vertical_align(VerticalAlign::Middle)
                .item_type(UIItemType::TabBar(item.item.clone()))
                .margin(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.),
                })
                .padding(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.25),
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .colors(ElementColors {
                    border: BorderColor::default(),
                    bg: new_tab.bg_color.to_linear().into(),
                    // "+" 图标默认蓝色
                    text: button_colors::icon_blue().into(),
                })
                .hover_colors(Some(ElementColors {
                    border: BorderColor::default(),
                    bg: new_tab_hover.bg_color.to_linear().into(),
                    text: button_colors::icon_blue_hover().into(),
                })),
                TabBarItem::Tab { active, .. } if active => element
                    .vertical_align(VerticalAlign::Bottom)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .border_corners(Some(Corners {
                        top_left: SizedPoly {
                            width: Dimension::Cells(0.5),
                            height: Dimension::Cells(0.5),
                            poly: TOP_LEFT_ROUNDED_CORNER,
                        },
                        top_right: SizedPoly {
                            width: Dimension::Cells(0.5),
                            height: Dimension::Cells(0.5),
                            poly: TOP_RIGHT_ROUNDED_CORNER,
                        },
                        bottom_left: SizedPoly::none(),
                        bottom_right: SizedPoly::none(),
                    }))
                    .colors(ElementColors {
                        border: BorderColor::new(
                            bg_color
                                .unwrap_or_else(|| active_tab.bg_color.into())
                                .to_linear(),
                        ),
                        bg: bg_color
                            .unwrap_or_else(|| active_tab.bg_color.into())
                            .to_linear()
                            .into(),
                        text: fg_color
                            .unwrap_or_else(|| active_tab.fg_color.into())
                            .to_linear()
                            .into(),
                    }),
                TabBarItem::Tab { .. } => element
                    .vertical_align(VerticalAlign::Bottom)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .border_corners(Some(Corners {
                        top_left: SizedPoly {
                            width: Dimension::Cells(0.5),
                            height: Dimension::Cells(0.5),
                            poly: TOP_LEFT_ROUNDED_CORNER,
                        },
                        top_right: SizedPoly {
                            width: Dimension::Cells(0.5),
                            height: Dimension::Cells(0.5),
                            poly: TOP_RIGHT_ROUNDED_CORNER,
                        },
                        bottom_left: SizedPoly {
                            width: Dimension::Cells(0.),
                            height: Dimension::Cells(0.33),
                            poly: &[],
                        },
                        bottom_right: SizedPoly {
                            width: Dimension::Cells(0.),
                            height: Dimension::Cells(0.33),
                            poly: &[],
                        },
                    }))
                    .colors({
                        let inactive_tab = colors.inactive_tab();
                        let bg = bg_color
                            .unwrap_or_else(|| inactive_tab.bg_color.into())
                            .to_linear();
                        let edge = colors.inactive_tab_edge().to_linear();
                        ElementColors {
                            border: BorderColor {
                                left: bg,
                                right: edge,
                                top: bg,
                                bottom: bg,
                            },
                            bg: bg.into(),
                            text: fg_color
                                .unwrap_or_else(|| inactive_tab.fg_color.into())
                                .to_linear()
                                .into(),
                        }
                    })
                    .hover_colors({
                        let inactive_tab_hover = colors.inactive_tab_hover();
                        Some(ElementColors {
                            border: BorderColor::new(
                                bg_color
                                    .unwrap_or_else(|| inactive_tab_hover.bg_color.into())
                                    .to_linear(),
                            ),
                            bg: bg_color
                                .unwrap_or_else(|| inactive_tab_hover.bg_color.into())
                                .to_linear()
                                .into(),
                            text: fg_color
                                .unwrap_or_else(|| inactive_tab_hover.fg_color.into())
                                .to_linear()
                                .into(),
                        })
                    }),
                TabBarItem::WindowButton(button) => window_button_element(
                    button,
                    self.window_state.contains(window::WindowState::MAXIMIZED),
                    &font,
                    &metrics,
                    &self.config,
                ),
                TabBarItem::ConfigUIButton => {
                    let new_tab = colors.new_tab();
                    let new_tab_hover = colors.new_tab_hover();
                    // iconfont 设置字形（U+E607），用专用图标字体渲染
                    Element::new(
                        &icon_font,
                        ElementContent::Text(iconfont_glyph::SETTINGS.to_string()),
                    )
                    .vertical_align(VerticalAlign::Middle)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .colors(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab.bg_color.to_linear().into(),
                        // 齿轮图标默认蓝色
                        text: button_colors::icon_blue().into(),
                    })
                    .hover_colors(Some(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab_hover.bg_color.to_linear().into(),
                        text: button_colors::icon_blue_hover().into(),
                    }))
                }
                TabBarItem::CopyScreenButton => {
                    let new_tab = colors.new_tab();
                    let new_tab_hover = colors.new_tab_hover();
                    // 复制成功反馈期内图标转绿色，1.5s 后由
                    // mouseevent.rs 的定时器清除状态恢复蓝色。
                    // 字形保持 iconfont 复制图标不变——iconfont 只有
                    // PUA 图标字形，✓(U+2713) 等字符在其中不可见。
                    let copied = self
                        .copy_button_feedback
                        .map_or(false, |t| t.elapsed() < Duration::from_millis(1500));
                    let icon_color = if copied {
                        button_colors::icon_green()
                    } else {
                        button_colors::icon_blue()
                    };
                    Element::new(
                        &icon_font,
                        ElementContent::Text(iconfont_glyph::COPY.to_string()),
                    )
                    .vertical_align(VerticalAlign::Middle)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .colors(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab.bg_color.to_linear().into(),
                        text: icon_color.into(),
                    })
                    .hover_colors(Some(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab_hover.bg_color.to_linear().into(),
                        text: if copied {
                            button_colors::icon_green()
                        } else {
                            button_colors::icon_blue_hover()
                        }
                        .into(),
                    }))
                }
                TabBarItem::SftpPanelButton => {
                    let new_tab = colors.new_tab();
                    let new_tab_hover = colors.new_tab_hover();
                    // iconfont SFTP 字形（U+E713）。SSH 标签:蓝色 + hover
                    // 提亮(与齿轮/复制同风格);非 SSH 标签:灰色不可用
                    let sftp_enabled = self.active_pane_is_ssh();
                    Element::new(
                        &icon_font,
                        ElementContent::Text(iconfont_glyph::SFTP.to_string()),
                    )
                    .vertical_align(VerticalAlign::Middle)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .colors(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab.bg_color.to_linear().into(),
                        text: if sftp_enabled {
                            button_colors::icon_blue()
                        } else {
                            button_colors::icon_dim()
                        }
                        .into(),
                    })
                    .hover_colors(Some(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab_hover.bg_color.to_linear().into(),
                        text: if sftp_enabled {
                            button_colors::icon_blue_hover()
                        } else {
                            button_colors::icon_dim()
                        }
                        .into(),
                    }))
                }
                TabBarItem::ClearScreenButton => {
                    let new_tab = colors.new_tab();
                    let new_tab_hover = colors.new_tab_hover();
                    // iconfont 清理字形（U+E641），蓝色 + hover 提亮
                    // （与齿轮/复制/SFTP 同风格）
                    Element::new(
                        &icon_font,
                        ElementContent::Text(iconfont_glyph::CLEAR.to_string()),
                    )
                    .vertical_align(VerticalAlign::Middle)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .colors(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab.bg_color.to_linear().into(),
                        text: button_colors::icon_blue().into(),
                    })
                    .hover_colors(Some(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab_hover.bg_color.to_linear().into(),
                        text: button_colors::icon_blue_hover().into(),
                    }))
                }
                TabBarItem::SendCommandButton => {
                    let new_tab = colors.new_tab();
                    let new_tab_hover = colors.new_tab_hover();
                    // iconfont 云命令行字形（U+E6E4），蓝色 + hover 提亮
                    // （与齿轮/复制/SFTP/清屏同风格）；按钮常显，指令为空时
                    // 点击打开设置窗体引导填写（mouseevent.rs）
                    Element::new(
                        &icon_font,
                        ElementContent::Text(iconfont_glyph::SEND.to_string()),
                    )
                    .vertical_align(VerticalAlign::Middle)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .colors(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab.bg_color.to_linear().into(),
                        text: button_colors::icon_blue().into(),
                    })
                    .hover_colors(Some(ElementColors {
                        border: BorderColor::default(),
                        bg: new_tab_hover.bg_color.to_linear().into(),
                        text: button_colors::icon_blue_hover().into(),
                    }))
                }
            }
        };

        // Reserve space for the native titlebar buttons
        if self
            .config
            .window_decorations
            .contains(::window::WindowDecorations::INTEGRATED_BUTTONS)
            && self.config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            && !self.window_state.contains(window::WindowState::FULL_SCREEN)
        {
            left_status.push(
                Element::new(&font, ElementContent::Text("".to_string())).margin(BoxDimension {
                    left: Dimension::Cells(4.0), // FIXME: determine exact width of macos ... buttons
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                }),
            );
        }

        for item in items {
            match item.item {
                TabBarItem::LeftStatus => left_status.push(item_to_elem(item)),
                TabBarItem::None | TabBarItem::RightStatus => right_eles.push(item_to_elem(item)),
                TabBarItem::WindowButton(_) => {
                    if self.config.integrated_title_button_alignment
                        == IntegratedTitleButtonAlignment::Left
                    {
                        left_eles.push(item_to_elem(item))
                    } else {
                        right_eles.push(item_to_elem(item))
                    }
                }
                TabBarItem::Tab { tab_idx, active } => {
                    let mut elem = item_to_elem(item);
                    elem.content = match elem.content {
                        ElementContent::Text(_) => unreachable!(),
                        ElementContent::Poly { .. } => unreachable!(),
                        ElementContent::Children(mut kids) => {
                            if self.config.show_close_tab_button_in_tabs {
                                kids.push(make_x_button(&font, &metrics, &colors, tab_idx, active));
                            }
                            ElementContent::Children(kids)
                        }
                    };
                    left_eles.push(elem);
                }
                // 新标签（+）按钮：有标签时跟随最后一个标签（符合「新建标签」
                // 操作习惯，参照 Windows Terminal/浏览器），零标签空窗口时
                // 回归右侧按钮组首位，避免孤悬最左缘
                TabBarItem::NewTabButton => new_tab_eles.push(item_to_elem(item)),
                // 其余变体即按钮组（齿轮/复制/SFTP/清屏/发送指令）：
                // 全部 push 进右侧浮动容器实现右对齐，push 顺序即屏幕上从左到右
                _ => right_eles.push(item_to_elem(item)),
            }
        }

        let border = self.get_os_border();
        let total_bar_width = (self.dimensions.pixel_width as f32
            - (border.left + border.right).get() as f32)
            .max(0.0);

        let layout_context = LayoutContext {
            height: DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.dimensions.pixel_height as f32,
                pixel_cell: metrics.cell_size.height as f32,
            },
            width: DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: total_bar_width,
                pixel_cell: metrics.cell_size.width as f32,
            },
            bounds: euclid::rect(
                border.left.get() as f32,
                0.,
                total_bar_width,
                tab_bar_height,
            ),
            metrics: &metrics,
            gl_state: self.render_state.as_ref().unwrap(),
            zindex: 10,
        };

        // + 按钮分流：有标签时并入左列尾部（紧随最后一个标签），零标签
        // 空窗口时插回右侧按钮组首位——无「最后一个标签」可依托，保持
        // 历史位置避免孤悬最左缘
        let num_tabs = left_eles
            .iter()
            .filter(|e| matches!(e.item_type, Some(UIItemType::TabBar(TabBarItem::Tab { .. }))))
            .count();
        let new_tab_follows_tabs = num_tabs > 0 && !new_tab_eles.is_empty();
        // 跟随标签流时先精确测量 + 按钮宽度（含 margin/padding/边框），
        // 供下方标签区间预留
        let new_tab_width = if new_tab_follows_tabs {
            let plus_probe =
                Element::new(&font, ElementContent::Children(new_tab_eles.clone()))
                    .colors(bar_colors.clone());
            self.compute_element(&layout_context, &plus_probe)?
                .bounds
                .width()
        } else {
            0.0
        };
        if new_tab_follows_tabs {
            left_eles.append(&mut new_tab_eles);
        } else {
            right_eles.splice(0..0, new_tab_eles.drain(..));
        }

        // 精确测量右侧按钮组（齿轮/复制/SFTP/清屏/发送指令/窗口按钮；零标签时含 +）的实际像素宽度
        let right_width = if right_eles.is_empty() {
            0.0
        } else {
            let right_probe = Element::new(
                &font,
                ElementContent::Children(right_eles.clone()),
            )
            .colors(bar_colors.clone());
            self.compute_element(&layout_context, &right_probe)?
                .bounds
                .width()
        };

        let left_status_width = if left_status.is_empty() {
            0.0
        } else {
            let left_probe = Element::new(
                &font,
                ElementContent::Children(left_status.clone()),
            )
            .colors(bar_colors.clone());
            self.compute_element(&layout_context, &left_probe)?
                .bounds
                .width()
        };

        let window_buttons_at_left = self
            .config
            .window_decorations
            .contains(window::WindowDecorations::INTEGRATED_BUTTONS)
            && (self.config.integrated_title_button_alignment
                == IntegratedTitleButtonAlignment::Left
                || self.config.integrated_title_button_style
                    == IntegratedTitleButtonStyle::MacOsNative);

        let left_padding = if window_buttons_at_left {
            if self.config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            {
                if !self.window_state.contains(window::WindowState::FULL_SCREEN) {
                    Dimension::Pixels(70.0)
                } else {
                    Dimension::Cells(0.5)
                }
            } else {
                Dimension::Pixels(0.0)
            }
        } else {
            Dimension::Cells(0.5)
        };

        let left_padding_px = left_padding.evaluate_as_pixels(layout_context.width);

        // 标签可用总像素区间：从左侧 padding / 状态文本之后，一直延伸到右侧按钮组左缘。
        // + 按钮已随标签流时其自身宽度同样预留——标签填满区间时 + 恰好补位在
        // 最后一个标签与按钮组之间。彻底消除标签与右侧按钮组之间的空白死区
        // （Issue/1.png 红框区域）。
        let available_tab_area = (total_bar_width
            - right_width
            - new_tab_width
            - left_padding_px
            - left_status_width)
            .max(0.0);

        if num_tabs > 0 {
            let cell_w = metrics.cell_size.width as f32;
            let tab_width = compute_tab_width(
                available_tab_area,
                num_tabs,
                self.config.tab_max_width,
                cell_w,
            );
            let border_and_padding_width = cell_w + 2.0;
            let content_width = (tab_width - border_and_padding_width).max(0.0);

            for elem in left_eles.iter_mut() {
                if matches!(elem.item_type, Some(UIItemType::TabBar(TabBarItem::Tab { .. }))) {
                    elem.min_width = Some(Dimension::Pixels(content_width));
                    elem.max_width = Some(Dimension::Pixels(tab_width));
                }
            }
        }

        let mut children = vec![];

        if !left_status.is_empty() {
            children.push(
                Element::new(&font, ElementContent::Children(left_status))
                    .colors(bar_colors.clone()),
            );
        }

        children.push(
            Element::new(&font, ElementContent::Children(left_eles))
                .vertical_align(VerticalAlign::Bottom)
                .colors(bar_colors.clone())
                .padding(BoxDimension {
                    left: left_padding,
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                })
                .zindex(1),
        );
        // zindex(2) 高于左侧标签容器的 zindex(1)：标签溢出时按钮组保持置顶
        // 可点击（ui_items 逆序命中也保证按钮优先于重叠的标签）
        children.push(
            Element::new(&font, ElementContent::Children(right_eles))
                .colors(bar_colors.clone())
                .float(Float::Right)
                .zindex(2),
        );

        let content = ElementContent::Children(children);

        let tabs = Element::new(&font, content)
            .display(DisplayType::Block)
            .item_type(UIItemType::TabBar(TabBarItem::None))
            .min_width(Some(Dimension::Pixels(self.dimensions.pixel_width as f32)))
            .min_height(Some(Dimension::Pixels(tab_bar_height)))
            .vertical_align(VerticalAlign::Bottom)
            .colors(bar_colors);

        let mut computed = self.compute_element(&layout_context, &tabs)?;

        computed.translate(euclid::vec2(
            0.,
            if self.config.tab_bar_at_bottom {
                self.dimensions.pixel_height as f32
                    - (computed.bounds.height() + border.bottom.get() as f32)
            } else {
                border.top.get() as f32
            },
        ));

        Ok(computed)
    }

    pub fn paint_fancy_tab_bar(&self) -> anyhow::Result<Vec<UIItem>> {
        let computed = self.fancy_tab_bar.as_ref().ok_or_else(|| {
            anyhow::anyhow!("paint_fancy_tab_bar called but fancy_tab_bar is None")
        })?;
        let ui_items = computed.ui_items();

        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)?;

        Ok(ui_items)
    }
}

fn make_x_button(
    font: &Rc<LoadedFont>,
    metrics: &RenderMetrics,
    colors: &TabBarColors,
    tab_idx: usize,
    active: bool,
) -> Element {
    Element::new(
        &font,
        ElementContent::Poly {
            line_width: metrics.underline_height.max(2),
            poly: SizedPoly {
                poly: X_BUTTON,
                width: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
                height: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
            },
        },
    )
    // Ensure that we draw our background over the
    // top of the rest of the tab contents
    .zindex(1)
    .vertical_align(VerticalAlign::Middle)
    .float(Float::Right)
    .item_type(UIItemType::CloseTab(tab_idx))
    .hover_colors({
        let inactive_tab_hover = colors.inactive_tab_hover();
        let active_tab = colors.active_tab();

        Some(ElementColors {
            border: BorderColor::default(),
            bg: (if active {
                inactive_tab_hover.bg_color
            } else {
                active_tab.bg_color
            })
            .to_linear()
            .into(),
            text: (if active {
                inactive_tab_hover.fg_color
            } else {
                active_tab.fg_color
            })
            .to_linear()
            .into(),
        })
    })
    .padding(BoxDimension {
        left: Dimension::Cells(0.25),
        right: Dimension::Cells(0.25),
        top: Dimension::Cells(0.25),
        bottom: Dimension::Cells(0.25),
    })
    .margin(BoxDimension {
        left: Dimension::Cells(0.5),
        right: Dimension::Cells(0.),
        top: Dimension::Cells(0.),
        bottom: Dimension::Cells(0.),
    })
}

/// 计算标签在可用空间内的宽度分配：
/// 1. 标签少时（如 1-3 个），宽度限制在舒适的上限（max_tab_px，默认约 180-240px），避免单个标签拉伸成巨大条状；
/// 2. 标签逐渐增多时（如 4-15 个），均匀填满标签区至右侧按钮区左缘，彻底消除空白死区（Issue/1.png 红框）；
/// 3. 标签很多时（如 20+ 个），保证每个标签不低于最小保底宽（min_tab_px = 36px），确保关闭按钮与内容可交互。
pub fn compute_tab_width(
    available_tab_area: f32,
    num_tabs: usize,
    tab_max_width_cells: usize,
    cell_w: f32,
) -> f32 {
    if num_tabs == 0 {
        return 0.0;
    }
    let ideal_tab_width = available_tab_area / num_tabs as f32;
    // 36/180/240/32 为 96 DPI、9px 基准 cell 宽下的设计值，按实际 cell 宽
    // 等比缩放：高 DPI 下上限/保底折合逻辑像素不漂移（金鱼盲审条件三）。
    // cell_w=9 时 scale=1，行为与历史版本逐像素一致。
    let scale = cell_w / 9.0;
    let max_tab_px = (tab_max_width_cells as f32 * cell_w + 32.0 * scale)
        .clamp(180.0 * scale, 240.0 * scale);
    let min_tab_px = 36.0 * scale;
    ideal_tab_width.clamp(min_tab_px, max_tab_px)
}

#[cfg(test)]
mod tests {
    use super::compute_tab_width;

    #[test]
    fn test_compute_tab_width_few_tabs_capped() {
        let available = 865.5;
        let cell_w = 9.0;
        let max_cells = 16;
        // 1 个标签：不应膨胀到 865.5px，应被上限 clamp（16 * 9 + 32 = 176px，clamp到180px）
        let w1 = compute_tab_width(available, 1, max_cells, cell_w);
        assert_eq!(w1, 180.0);
    }

    #[test]
    fn test_compute_tab_width_issue_1_twelve_tabs_fills_area() {
        // Issue/1.png 场景：865.5px 可用区间，12 个标签
        let available = 865.5;
        let cell_w = 9.0;
        let max_cells = 16;
        let w12 = compute_tab_width(available, 12, max_cells, cell_w);
        // 865.5 / 12 = 72.125px
        assert!((w12 - 72.125).abs() < 1e-4);
        // 12 个标签总宽覆盖全部可用空间，与右侧按钮无缝衔接
        let total = w12 * 12.0;
        assert!((total - available).abs() < 1e-4);
    }

    #[test]
    fn test_compute_tab_width_overflow_tabs_guarantees_min() {
        // 极多标签：30 个标签在 865.5px，理想宽 28.85px，保底为 36.0px
        let available = 865.5;
        let cell_w = 9.0;
        let max_cells = 16;
        let w30 = compute_tab_width(available, 30, max_cells, cell_w);
        assert_eq!(w30, 36.0);
    }

    #[test]
    fn test_compute_tab_width_scales_with_cell_width() {
        // 高 DPI（cell_w=18，200%）：全部物理像素常数等比翻倍，
        // 折合逻辑像素与 96 DPI 基准一致（金鱼盲审条件三）
        let available = 865.5;
        let max_cells = 16;
        let w1 = compute_tab_width(available, 1, max_cells, 18.0);
        assert_eq!(w1, 360.0, "上限 clamp 应随 cell 宽翻倍（180→360px）");
        let w30 = compute_tab_width(available, 30, max_cells, 18.0);
        assert_eq!(w30, 72.0, "保底宽应随 cell 宽翻倍（36→72px）");
        // 逻辑语义不变：360/2=180px、72/2=36px
    }
}
