//! orca-term 配置读写核心：读链路（Lua → Config → JSON 快照）与写链路（表单 → Lua / 备份）。
//! 产品决策：配置文件由配置界面全量管理，不提供手写 Lua 入口（无标记区概念）。

pub mod backup;
pub mod form;
pub mod keybinds;
pub mod load;
pub mod schemes;
pub mod settings;
pub mod ssh;
