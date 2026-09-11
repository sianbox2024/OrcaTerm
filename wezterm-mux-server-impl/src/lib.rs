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
            // 热重载:同名域已存在。配置无变化则跳过;有变化时,
            // 仅在域上没有活动 pane 的情况下用新配置重建(RemoteSshDomain
            // 创建时克隆配置快照,spawn 用的是快照而非实时配置,不重建
            // 则改 ssh_domains 保存后对运行中的 GUI 永不生效)。
            if let Some(ssh_domain) = existing.downcast_ref::<RemoteSshDomain>() {
                if ssh_domain.ssh_domain_config() == &ssh_dom {
                    continue;
                }
                let domain_id = existing.domain_id();
                if mux.domain_has_active_panes(domain_id) {
                    log::warn!(
                        "config for ssh domain `{}` changed, but it has active panes; \
                         the new config takes effect after those panes close, or restart",
                        ssh_dom.name
                    );
                    continue;
                }
                mux.remove_domain(&existing);
            } else {
                // 同名但不是 RemoteSshDomain(理论上不该发生),保守起见跳过
                continue;
            }
        }

        let domain: Arc<dyn Domain> = Arc::new(RemoteSshDomain::with_ssh_domain(&ssh_dom)?);
        mux.add_domain(&domain);
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

        Mux::shutdown();
    }
}
