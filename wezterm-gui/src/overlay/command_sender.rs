//! 标签栏「发送指令」按钮（U+E6E4）的指令选择弹层：列出
//! `config.tab_button_commands` 的全部条目（label 为显示文本，command
//! 为发送内容），Enter / 鼠标左键点击选中即向打开弹层时的真实 pane
//! 发送 `command + \r`（等价键入后回车），Esc 取消。
//! 克隆自 launcher.rs 并裁剪：去掉 domains/tabs/workspaces/命令面板等
//! 条目来源与字母快捷标签，仅保留指令列表 + 模糊过滤 + 键盘/鼠标选择。
use crate::overlay::selector::{matcher_pattern, matcher_score};
use crate::termwindow::TermWindowNotif;
use config::configuration;
use config::keyassignment::{KeyAssignment, TabButtonCommand};
use mux::pane::PaneId;
use mux::termwiztermtab::TermWizTerminal;
use rayon::prelude::*;
use std::time::Duration;
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use termwiz_funcs::truncate_right;
use window::WindowOps;

#[derive(Clone)]
pub struct CommandSenderArgs {
    pub pane_id: PaneId,
    pub title: String,
    pub help_text: String,
    pub fuzzy_help_text: String,
}

const ROW_OVERHEAD: usize = 3;

#[derive(Debug, Clone)]
struct Entry {
    /// 列表显示文本（配置 label 留空时回退为 command）
    label: String,
    /// 选中后发送的指令文本（发送时自动补 \r）
    command: String,
}

fn build_entries(cmds: &[TabButtonCommand]) -> Vec<Entry> {
    cmds.iter()
        .map(|item| {
            let label = if item.label.trim().is_empty() {
                item.command.clone()
            } else {
                item.label.clone()
            };
            Entry {
                label,
                command: item.command.clone(),
            }
        })
        .collect()
}

struct CommandSenderState {
    active_idx: usize,
    max_items: usize,
    top_row: usize,
    entries: Vec<Entry>,
    filter_term: String,
    filtered_entries: Vec<Entry>,
    pane_id: PaneId,
    window: ::window::Window,
    filtering: bool,
    help_text: String,
    fuzzy_help_text: String,
    /// 构建 entries 时的配置代次；变化即在 run_loop 中重建条目
    ///（弹层挂着时在设置界面新增/修改指令并保存 → 热重载即时生效）
    config_generation: usize,
}

impl CommandSenderState {
    fn update_filter(&mut self) {
        if self.filter_term.is_empty() {
            self.filtered_entries = self.entries.clone();
            return;
        }

        self.filtered_entries.clear();

        let pattern = matcher_pattern(&self.filter_term);

        struct MatchResult {
            row_idx: usize,
            score: u32,
        }

        let mut scores: Vec<MatchResult> = self
            .entries
            .par_iter()
            .enumerate()
            .filter_map(|(row_idx, entry)| {
                // label 与 command 任一命中即入选，取较高分排序
                let score = match (
                    matcher_score(&pattern, &entry.label),
                    matcher_score(&pattern, &entry.command),
                ) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (Some(a), None) | (None, Some(a)) => Some(a),
                    (None, None) => None,
                }?;
                Some(MatchResult { row_idx, score })
            })
            .collect();

        scores.sort_by(|a, b| a.score.cmp(&b.score).reverse());

        for result in scores {
            self.filtered_entries
                .push(self.entries[result.row_idx].clone());
        }

        self.active_idx = 0;
        self.top_row = 0;
    }

    fn render(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        let max_width = size.cols.saturating_sub(6);
        let max_items = size.rows.saturating_sub(ROW_OVERHEAD);
        self.max_items = max_items;

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            Change::Text(format!(
                "{}\r\n",
                truncate_right(&self.help_text, max_width)
            )),
            Change::AllAttributes(CellAttributes::default()),
        ];

        for (row_num, (entry_idx, entry)) in self
            .filtered_entries
            .iter()
            .enumerate()
            .skip(self.top_row)
            .enumerate()
        {
            if row_num > max_items {
                break;
            }

            let mut attr = CellAttributes::blank();
            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(true).into());
                attr.set_reverse(true);
            }

            // 主文本：label（build_entries 已保证非空）
            let mut line = crate::tabbar::parse_status_text(&entry.label, attr.clone());
            if line.len() > max_width {
                line.resize(max_width, termwiz::surface::SEQ_ZERO);
            }
            let used = line.len();
            changes.append(&mut line.changes(&attr));

            // 副文本：label 与 command 不同时，以半亮色在行尾补显真实指令，
            // 便于确认将要发送的内容
            if !entry.command.is_empty() && entry.label != entry.command {
                let avail = max_width.saturating_sub(used + 2);
                if avail > 0 {
                    let hint = truncate_right(&format!("  {}", entry.command), avail);
                    let mut cattr = attr.clone();
                    cattr.set_intensity(Intensity::Half);
                    let mut hint_line = crate::tabbar::parse_status_text(&hint, cattr.clone());
                    if hint_line.len() > avail {
                        hint_line.resize(avail, termwiz::surface::SEQ_ZERO);
                    }
                    changes.append(&mut hint_line.changes(&cattr));
                }
            }

            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(false).into());
            }
            changes.push(Change::AllAttributes(CellAttributes::default()));
            changes.push(Change::Text("\r\n".to_string()));
        }

        if self.filtering || !self.filter_term.is_empty() {
            changes.append(&mut vec![
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::ClearToEndOfLine(ColorAttribute::Default),
                Change::Text(truncate_right(
                    &format!("{}{}", &self.fuzzy_help_text, self.filter_term),
                    max_width,
                )),
            ]);
        }

        term.render(&changes)
    }

    /// 选中条目：向打开弹层时捕获的真实 pane 发送 `command + \r`
    ///（KeyAssignment::SendString 由 TermWindow 既有分派写入 pane writer）。
    /// 返回是否成功命中条目，调用方据此决定是否退出弹层。
    fn launch(&mut self, active_idx: usize) -> bool {
        if let Some(entry) = self.filtered_entries.get(active_idx) {
            let assignment = KeyAssignment::SendString(format!("{}\r", entry.command));
            self.window.notify(TermWindowNotif::PerformAssignment {
                pane_id: self.pane_id,
                assignment,
                tx: None,
            });
            true
        } else {
            false
        }
    }

    fn move_up(&mut self) {
        self.active_idx = self.active_idx.saturating_sub(1);
        if self.active_idx < self.top_row {
            self.top_row = self.active_idx;
        }
    }

    fn move_down(&mut self) {
        self.active_idx = (self.active_idx + 1).min(self.filtered_entries.len() - 1);
        if self.active_idx > self.top_row + self.max_items {
            self.top_row = self.active_idx.saturating_sub(self.max_items);
        }
    }

    /// 配置重载后重建条目（弹层挂着时在设置界面编辑指令并保存 → 热重载
    /// 代次变化 → 新列表即时生效）。返回是否发生了重建。
    fn reload_entries_if_config_changed(&mut self) -> bool {
        let generation = configuration().generation();
        if generation == self.config_generation {
            return false;
        }
        self.config_generation = generation;
        self.entries = build_entries(&configuration().effective_tab_button_commands());
        self.update_filter();
        // 光标位置尽量保留，clamp 到新列表范围内；空列表时归零
        self.active_idx = self
            .active_idx
            .min(self.filtered_entries.len().saturating_sub(1));
        self.top_row = self
            .top_row
            .min(self.filtered_entries.len().saturating_sub(1));
        true
    }

    fn run_loop(&mut self, term: &mut TermWizTerminal) -> anyhow::Result<()> {
        // 带超时轮询输入：空闲时醒来检测配置重载（generation 变化即重建条目），
        // 否则弹层挂着期间保存配置永远看不到新列表
        while let Ok(event) = term.poll_input(Some(Duration::from_millis(250))) {
            let event = match event {
                Some(event) => event,
                None => {
                    if self.reload_entries_if_config_changed() {
                        self.render(term)?;
                    }
                    continue;
                }
            };
            match event {
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('j'),
                    ..
                }) if !self.filtering => {
                    self.move_down();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('k'),
                    ..
                }) if !self.filtering => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('P' | 'K'),
                    modifiers: Modifiers::CTRL,
                }) => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('N' | 'J'),
                    modifiers: Modifiers::CTRL,
                }) => {
                    self.move_down();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('/'),
                    ..
                }) if !self.filtering => {
                    self.filtering = true;
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Backspace,
                    ..
                }) => {
                    if self.filtering {
                        if self.filter_term.pop().is_none() {
                            self.filtering = false;
                        }
                        self.update_filter();
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('G') | KeyCode::Char('['),
                    modifiers: Modifiers::CTRL,
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                }) => {
                    break;
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char(c),
                    ..
                }) if self.filtering => {
                    self.filter_term.push(c);
                    self.update_filter();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::UpArrow,
                    ..
                }) => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::DownArrow,
                    ..
                }) => {
                    self.move_down();
                }
                InputEvent::Mouse(MouseEvent {
                    y, mouse_buttons, ..
                }) if mouse_buttons.contains(MouseButtons::VERT_WHEEL) => {
                    if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                        self.top_row = self.top_row.saturating_sub(1);
                    } else {
                        self.top_row += 1;
                        self.top_row = self.top_row.min(
                            self.filtered_entries
                                .len()
                                .saturating_sub(self.max_items)
                                .saturating_sub(1),
                        );
                    }
                    if y > 0 && y as usize <= self.filtered_entries.len() {
                        self.active_idx = self.top_row + y as usize - 1;
                    }
                }
                InputEvent::Mouse(MouseEvent {
                    y, mouse_buttons, ..
                }) => {
                    if y > 0 && y as usize <= self.filtered_entries.len() {
                        self.active_idx = self.top_row + y as usize - 1;

                        if mouse_buttons == MouseButtons::LEFT {
                            if self.launch(self.active_idx) {
                                break;
                            }
                        }
                    }
                    if mouse_buttons != MouseButtons::NONE {
                        // 其他鼠标键视为取消
                        break;
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Enter,
                    ..
                }) => {
                    if self.launch(self.active_idx) {
                        break;
                    }
                }
                _ => {}
            }
            self.render(term)?;
        }

        Ok(())
    }
}

/// 弹层入口（经 start_overlay 在独立线程运行）：渲染指令列表并进入
/// 交互循环，函数返回即由 start_overlay 自动取消 overlay、焦点回到
/// 原 pane。
pub fn command_sender(
    args: CommandSenderArgs,
    mut term: TermWizTerminal,
    window: ::window::Window,
) -> anyhow::Result<()> {
    let config_generation = configuration().generation();
    let mut state = CommandSenderState {
        active_idx: 0,
        max_items: 0,
        pane_id: args.pane_id,
        top_row: 0,
        entries: build_entries(&configuration().effective_tab_button_commands()),
        filter_term: String::new(),
        filtered_entries: vec![],
        window,
        filtering: false,
        help_text: args.help_text.clone(),
        fuzzy_help_text: args.fuzzy_help_text.clone(),
        config_generation,
    };

    term.set_raw_mode()?;
    term.render(&[Change::Title(args.title.to_string())])?;
    state.update_filter();
    state.render(&mut term)?;
    state.run_loop(&mut term)
}
