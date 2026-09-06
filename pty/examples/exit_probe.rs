//! 临时探针 v2：ConPTY 里跑 powershell.exe，等 pwsh7 提示符就绪后发 exit，
//! 增量落盘全部原始字节，并记录子进程退出时序（排查退出乱码/退不掉问题）。
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn now(t0: Instant) -> String {
    format!("[{:6.2}s]", t0.elapsed().as_secs_f64())
}

fn main() {
    let t0 = Instant::now();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new("powershell.exe");
    cmd.arg("-NoLogo");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    println!("{} spawned powershell.exe pid={:?}", now(t0), child.process_id());

    // 读线程：增量写 output.bin，并维护累计字节数
    let total = Arc::new(AtomicUsize::new(0));
    let total2 = total.clone();
    let mut reader = pair.master.try_clone_reader().unwrap();
    std::thread::spawn(move || {
        let mut f =
            std::fs::File::create(r"D:\MyProjects\orca-term\Temp\pty-exit-probe\output.bin")
                .unwrap();
        let mut chunk = [0u8; 65536];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    f.write_all(&chunk[..n]).unwrap();
                    f.flush().unwrap();
                    total2.fetch_add(n, Ordering::SeqCst);
                }
                Err(_) => break,
            }
        }
    });

    let mut writer = pair.master.take_writer().unwrap();

    // 阶段1：等 pwsh7 就绪（starship 提示符含 U+276F ❯ 或输出稳定增长）
    // 简化：轮询输出里是否出现 "probe" 之前的 starship 段太脆，改为固定等待 + PING 验证
    std::thread::sleep(Duration::from_secs(12));
    let before = total.load(Ordering::SeqCst);
    writer.write_all(b"echo PING-MARKER\r").unwrap();
    writer.flush().unwrap();
    println!("{} sent PING", now(t0));
    // 等 PING 回显出现在输出流里（最多 30s）
    let mut ping_ok = false;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(500));
        let data = std::fs::read(r"D:\MyProjects\orca-term\Temp\pty-exit-probe\output.bin").unwrap();
        if String::from_utf8_lossy(&data).contains("PING-MARKER") {
            ping_ok = true;
            break;
        }
    }
    println!("{} PING echoed: {} (bytes {} -> {})", now(t0), ping_ok, before, total.load(Ordering::SeqCst));
    if !ping_ok {
        println!("!! PING never echoed; shell chain not interactive. Dumping tail anyway.");
    }

    // 阶段2：发 exit，观察子进程是否退出
    std::thread::sleep(Duration::from_secs(3));
    writer.write_all(b"exit\r").unwrap();
    writer.flush().unwrap();
    println!("{} sent exit", now(t0));

    let mut exited = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(Some(s)) = child.try_wait() {
            println!("{} CHILD EXITED status={:?}", now(t0), s);
            exited = true;
            break;
        }
    }
    if !exited {
        println!("{} child still alive 20s after exit -> KILLING", now(t0));
        // 杀之前先看输出尾部（交接瞬间的字节）
        let _ = child.kill();
        let _ = child.wait();
    }

    std::thread::sleep(Duration::from_secs(2));
    let bytes_at_end = total.load(Ordering::SeqCst);
    drop(pair.master);
    // 等读线程收尾
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(500));
        if total.load(Ordering::SeqCst) == bytes_at_end {
            break;
        }
    }
    println!("{} done, total bytes {}", now(t0), total.load(Ordering::SeqCst));
}
