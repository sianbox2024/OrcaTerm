//! orca-term 配置读写核心：读链路（Lua → Config → JSON 快照）与写链路（标记区切分/重生成/备份）。

pub mod backup;
pub mod form;
pub mod keybinds;
pub mod load;
pub mod markers;
pub mod schemes;
pub mod settings;
pub mod ssh;
