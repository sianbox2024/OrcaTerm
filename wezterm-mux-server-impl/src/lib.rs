use config::{ConfigHandle, SshMultiplexing};
use mux::domain::{Domain, LocalDomain};
use mux::ssh::RemoteSshDomain;
use mux::Mux;
use std::sync::Arc;
use wezterm_client::domain::{ClientDomain, ClientDomainConfig};

pub mod dispatch;
pub mod local;
pub mod pki;
pub mod sessionhandler;

fn client_domains(config: &config::ConfigHandle) -> Vec<ClientDomainConfig> {
    let mut domains = vec![];
    for unix_dom in &config.unix_domains {
        domains.push(ClientDomainConfig::Unix(unix_dom.clone()));
    }

    for ssh_dom in config.ssh_domains().into_iter() {
        if ssh_dom.multiplexing == SshMultiplexing::WezTerm {
            domains.push(ClientDomainConfig::Ssh(ssh_dom.clone()));
        }
    }

    for tls_client in &config.tls_clients {
        domains.push(ClientDomainConfig::Tls(tls_client.clone()));
    }
    domains
}

pub fn update_mux_domains(config: &ConfigHandle) -> anyhow::Result<()> {
    update_mux_domains_impl(config, false)
}

pub fn update_mux_domains_for_server(config: &ConfigHandle) -> anyhow::Result<()> {
    update_mux_domains_impl(config, true)
}

fn update_mux_domains_impl(config: &ConfigHandle, is_standalone_mux: bool) -> anyhow::Result<()> {
    let mux = Mux::get();

    for client_config in client_domains(&config) {
        if mux.get_domain_by_name(client_config.name()).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(ClientDomain::new(client_config));
        mux.add_domain(&domain);
    }

    for ssh_dom in config.ssh_domains().into_iter() {
        if ssh_dom.multiplexing != SshMultiplexing::None {
            continue;
        }

        if let Some(existing) = mux.get_domain_by_name(&ssh_dom.name) {
            // 热重载:同名域已存在。配置无变化则跳过;有变化时用携带新
            // 快照的新域对象重建(RemoteSshDomain 创建时克隆配置快照,
            // spawn 用的是快照而非实时配置,不重建则改 ssh_domains 保存后
            // 对运行中的 GUI 永不生效)。
            if let Some(ssh_domain) = existing.downcast_ref::<RemoteSshDomain>() {
                if ssh_domain.ssh_domain_config() == &ssh_dom {
                    continue;
                }
                let domain_id = existing.domain_id();
                if mux.domain_has_active_panes(domain_id) {
                    // 旧域仍有活动 pane:不能从按 id 表摘除(旧 pane 的
                    // split 等操作按 id 解析域),但也不再阻止重建——下面的
                    // add_domain 会覆盖同名映射,新连接(按名称解析)立即用
                    // 新配置;旧域对象留在按 id 表中,活动 pane 的会话与
                    // 分屏完全不受影响。
                    // (此前此处是 warn+continue:重建只在保存那一刻尝试
                    // 一次,之后 pane 关闭再连接用的仍是旧快照,只有重启
                    // GUI 才能拿到新配置——用户实测「连接后命令」保存后
                    // 一直执行旧命令,重启才生效。)
                    log::info!(
                        "config for ssh domain `{}` changed; existing panes keep \
                         their session, new connections use the new config",
                        ssh_dom.name
                    );
                } else {
                    // 无活动 pane:顺手把旧域从按 id 表摘除,防止反复改
                    // 配置积累死对象。
                    mux.remove_domain(&existing);
                }
            } else {
                // 同名但不是 RemoteSshDomain(理论上不该发生),保守起见跳过
                continue;
            }
        }

        let domain: Arc<dyn Domain> = Arc::new(RemoteSshDomain::with_ssh_domain(&ssh_dom)?);
        mux.add_domain(&domain);
        // 排障锚点：启动注册与热重载注册都走这里，配合 config reload 日志可判定
        // 「菜单里已有 SSH 项但点击无反应」时域是否已注册。
        log::info!("registered ssh domain `{}`", ssh_dom.name);
    }

    for wsl_dom in config.wsl_domains() {
        if mux.get_domain_by_name(&wsl_dom.name).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new_wsl(wsl_dom.clone())?);
        mux.add_domain(&domain);
    }

    for exec_dom in &config.exec_domains {
        if mux.get_domain_by_name(&exec_dom.name).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new_exec_domain(exec_dom.clone())?);
        mux.add_domain(&domain);
    }

    for serial in &config.serial_ports {
        if mux.get_domain_by_name(&serial.name).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new_serial_domain(serial.clone())?);
        mux.add_domain(&domain);
    }

    if is_standalone_mux {
        if let Some(name) = &config.default_mux_server_domain {
            if let Some(dom) = mux.get_domain_by_name(name) {
                if dom.is::<ClientDomain>() {
                    anyhow::bail!("default_mux_server_domain cannot be set to a client domain!");
                }
                mux.set_default_domain(&dom);
            }
        }
    } else {
        if let Some(name) = &config.default_domain {
            if let Some(dom) = mux.get_domain_by_name(name) {
                mux.set_default_domain(&dom);
            }
        }
    }

    Ok(())
}

lazy_static::lazy_static! {
    pub static ref PKI: pki::Pki = pki::Pki::init().expect("failed to initialize PKI");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 热重载同名域更新:update_mux_domains 在 ssh_domains 配置变化时
    /// 应替换旧域(无活动 pane);配置未变时保留既有域对象。
    /// 修复前:同名域无条件 continue,RemoteSshDomain 的配置快照
    /// (连接后命令 default_prog)永不更新——改配置保存后对运行中的
    /// GUI 不生效。
    #[test]
    fn ssh_domain_hot_reload_replaces_changed_domain() {
        let mux = Arc::new(Mux::new(None));
        Mux::set_mux(&mux);

        let make_config = |prog: &str| {
            let mut cfg = config::Config::default_config();
            cfg.ssh_domains = Some(vec![config::SshDomain {
                name: "testssh".to_string(),
                remote_address: "127.0.0.1:22".to_string(),
                username: Some("u".to_string()),
                default_prog: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    prog.to_string(),
                ]),
                // 注意:Rust 侧 Default 是 WezTerm,会被归入 ClientDomain;
                // 本测试针对 multiplexing=None 的 RemoteSshDomain 路径
                multiplexing: SshMultiplexing::None,
                ..Default::default()
            }]);
            config::use_this_configuration(cfg);
            config::configuration()
        };

        // 第一轮:创建域
        let cfg1 = make_config("cd /old/dir; exec $SHELL");
        update_mux_domains(&cfg1).unwrap();
        let dom1 = mux.get_domain_by_name("testssh").expect("domain created");
        let dom1_id = dom1.domain_id();

        // 第二轮:同名但配置不变 → 保留同一对象
        update_mux_domains(&cfg1).unwrap();
        let dom_unchanged = mux.get_domain_by_name("testssh").unwrap();
        assert_eq!(
            dom_unchanged.domain_id(),
            dom1_id,
            "unchanged config must keep domain"
        );

        // 第三轮:配置变化(连接后命令改了)→ 域被替换为新对象
        let cfg2 = make_config("cd /new/dir; exec $SHELL");
        update_mux_domains(&cfg2).unwrap();
        let dom2 = mux.get_domain_by_name("testssh").expect("domain after reload");
        assert_ne!(
            dom2.domain_id(),
            dom1_id,
            "changed config must replace domain"
        );
        // 新域持有新配置快照
        let ssh = dom2
            .downcast_ref::<RemoteSshDomain>()
            .expect("is RemoteSshDomain");
        assert_eq!(
            ssh.ssh_domain_config().default_prog,
            Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "cd /new/dir; exec $SHELL".to_string()
            ]),
            "new domain must carry new default_prog"
        );
        // 旧域无活动 pane:已被从按 id 表清理
        assert!(
            mux.get_domain(dom1_id).is_none(),
            "stale domain without panes must be dropped"
        );

        // 第四轮:换绑语义(旧域仍有活动 pane 时的路径)——不摘旧域、直接
        // 注册同名新域:update_mux_domains 对有活动 pane 的域正是这么做的。
        let dom3: Arc<dyn Domain> = Arc::new(
            RemoteSshDomain::with_ssh_domain(&make_config("cd /dir3; exec $SHELL").ssh_domains()[0])
                .unwrap(),
        );
        mux.add_domain(&dom3);
        // 同名映射已被覆盖:按名称拿到的是新域(新连接用新配置)
        assert_eq!(
            mux.get_domain_by_name("testssh").unwrap().domain_id(),
            dom3.domain_id(),
            "name mapping must point to the newest domain"
        );
        // 旧域(dom2)仍可按 id 解析:旧 pane 的 split 等操作不受影响
        assert!(
            mux.get_domain(dom2.domain_id()).is_some(),
            "old domain must stay resolvable by id for its active panes"
        );

        Mux::shutdown();
    }
}
