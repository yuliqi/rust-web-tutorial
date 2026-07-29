//! PTY 桥接：打开一个伪终端、spawn 一个进程，并把它的阻塞式读/写句柄桥接成
//! 异步任务能用的 channel。这是「浏览器里操作真终端」的底座。
//!
//! ## PTY vs 直接 pipe：为什么必须是伪终端
//!
//! 最朴素的想法是 `Command::new("bash")` 加 `Stdio::piped()`，把 stdin/stdout 接管道。
//! 但那样得到的是**非交互**的哑管道，很多东西会坏掉：
//! - 程序用 `isatty()` 判断「输出是不是终端」，是管道时会**关掉颜色、关掉分页、
//!   走批处理模式**（`ls` 不上色、`git log` 不进 pager）；
//! - 没有行编辑（退格、Ctrl-A/E、方向键历史都失效）；
//! - 没有信号（Ctrl-C 送 SIGINT、Ctrl-Z 送 SIGTSTP 靠的是终端的 line discipline）；
//! - 没有**窗口尺寸**概念，`vim`/`top` 这类全屏程序不知道该画多大，且窗口变化时
//!   收不到 SIGWINCH。
//!
//! **伪终端（PTY）** 是内核提供的一对虚拟设备：master 端在我们手里（读程序输出、
//! 写用户输入、设尺寸），slave 端作为子进程的「控制终端」。对子进程而言它和真实
//! 终端毫无区别——这才是「真终端体验」。堡垒机要录的、要重放的，正是这条 PTY 上
//! 的字节流。
//!
//! ## 阻塞 IO 与异步的桥接
//!
//! portable-pty 给的 reader/writer 是**阻塞**的 `Read`/`Write`。直接在 async 任务里
//! 调用会把整个 tokio worker 线程堵死。惯用解法：把阻塞读写放进**独立的 OS 线程**
//! （长期存活的阻塞循环，用 `std::thread` 而非 `spawn_blocking`），线程与异步侧之间
//! 用 channel 传字节。于是 ws.rs 里就只跟两个 channel 打交道，看不到任何阻塞调用。

use std::io::{Read, Write};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// PTY 里要 spawn 的命令。真实堡垒机这里是「ssh 到目标机」，教学默认是本机 shell。
#[derive(Debug, Clone)]
pub struct PtyConfig {
    pub program: String,
    pub args: Vec<String>,
}

impl PtyConfig {
    /// 本机登录 shell：优先 `$SHELL`，否则退到 `/bin/sh`（Windows 退到 cmd.exe）。
    /// ⚠️ 真实堡垒机**绝不**在跳板机本地开 shell——那等于把跳板机自己交出去。
    /// 正确做法是这里 spawn `ssh user@目标机`（或用 russh 之类在进程内做 SSH 代理），
    /// 并叠加命令黑白名单。见文件末尾「堡垒机骨架 vs 生产」。
    pub fn login_shell() -> Self {
        #[cfg(windows)]
        let default = "cmd.exe".to_string();
        #[cfg(not(windows))]
        let default = "/bin/sh".to_string();
        let program = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or(default);
        Self {
            program,
            args: Vec::new(),
        }
    }

    /// 指定命令 + 参数。测试用它跑确定性命令（如 `/bin/echo hello`）。
    pub fn program(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

/// 终端窗口尺寸。默认 24 行 80 列是终端的传统缺省值。
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

/// 纯逻辑：把（可能来自客户端、可能是 0 或异常大的）行列数收敛到合法范围。
///
/// 抽成独立函数是为了**能离线单测**：0 行 0 列会让 `openpty`/`resize` 直接报错，
/// 而客户端发来的 resize 消息完全不可信（可能是 0，也可能是恶意的巨大值想撑爆内存），
/// 所以在喂给内核前必须 clamp。这类「不可信输入的边界处理」是安全代码的常规动作。
pub fn clamp_size(rows: u16, cols: u16) -> (u16, u16) {
    let rows = rows.clamp(1, 1000);
    let cols = cols.clamp(1, 1000);
    (rows, cols)
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    let (rows, cols) = clamp_size(rows, cols);
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// 从异步侧发给 PTY 控制线程的指令。区分「数据」（Input）与「控制」（Resize/Shutdown）——
/// 终端协议里这两类必须分开，见 ws.rs 的协议说明。
#[derive(Debug)]
pub enum PtyControl {
    /// 用户输入的原始字节，写入 PTY（子进程从它的 stdin 读到）。
    Input(Vec<u8>),
    /// 窗口尺寸变化。全屏程序（vim/top）靠它重绘。
    Resize { rows: u16, cols: u16 },
    /// 主动结束：杀掉子进程并收尾。
    Shutdown,
}

/// PTY 桥接句柄：异步侧只跟这两个 channel 打交道。
/// - `control`：往里发 [`PtyControl`]（输入、resize、shutdown）；
/// - `output`：从里收 PTY 输出的字节块，`None` 表示 PTY 已结束（子进程退出）。
pub struct PtyBridge {
    control: UnboundedSender<PtyControl>,
    output: UnboundedReceiver<Vec<u8>>,
}

impl PtyBridge {
    /// 打开 PTY、spawn 命令、拉起读线程与控制线程。
    pub fn spawn(config: &PtyConfig, rows: u16, cols: u16) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(pty_size(rows, cols))
            .context("open pty")?;

        // 构造要执行的命令。设 TERM 让子进程知道自己在一个支持颜色的终端里。
        let mut cmd = CommandBuilder::new(&config.program);
        for arg in &config.args {
            cmd.arg(arg);
        }
        cmd.env("TERM", "xterm-256color");

        // slave 端交给子进程当控制终端；spawn 后我们不再需要 slave 句柄，及时 drop，
        // 否则「所有 slave 句柄都关闭」这个 EOF 条件永远不成立，读端可能永远等不到 EOF。
        let child = pair.slave.spawn_command(cmd).context("spawn command in pty")?;
        drop(pair.slave);

        // master 端：一个读句柄（克隆出来给读线程）、一个写句柄，外加 master 本身用于 resize。
        let mut reader = pair.master.try_clone_reader().context("clone pty reader")?;
        let mut writer = pair.master.take_writer().context("take pty writer")?;
        let master = pair.master;

        let (output_tx, output_rx) = unbounded_channel::<Vec<u8>>();
        let (control_tx, mut control_rx) = unbounded_channel::<PtyControl>();

        // —— 读线程：把 PTY 输出源源不断读出来推给异步侧 ——
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,            // EOF：子进程退出、PTY 关闭
                    Ok(n) => {
                        // 接收端已关闭（ws 连接断了）就没必要再读，退出。
                        if output_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,           // 读错误：当作结束处理
                }
            }
            // 线程结束 → output_tx drop → 异步侧 output.recv() 得到 None，得知 PTY 已结束。
        });

        // —— 控制线程：拥有 writer 与 master，串行处理输入/resize/shutdown ——
        // 单线程串行保证写入顺序，也避免对 writer/master 的并发访问。
        std::thread::spawn(move || {
            let mut child = child;
            // blocking_recv：在普通 OS 线程里同步地等 tokio channel。
            while let Some(ctrl) = control_rx.blocking_recv() {
                match ctrl {
                    PtyControl::Input(bytes) => {
                        if writer.write_all(&bytes).is_err() {
                            break;
                        }
                        let _ = writer.flush();
                    }
                    PtyControl::Resize { rows, cols } => {
                        let _ = master.resize(pty_size(rows, cols));
                    }
                    PtyControl::Shutdown => {
                        let _ = child.kill();
                        break;
                    }
                }
            }
            // 控制通道关闭（ws 任务结束、control_tx 被 drop）也走到这里：确保子进程被回收。
            let _ = child.kill();
            let _ = child.wait();
        });

        Ok(Self {
            control: control_tx,
            output: output_rx,
        })
    }

    /// 写用户输入。
    pub fn write_input(&self, bytes: Vec<u8>) {
        let _ = self.control.send(PtyControl::Input(bytes));
    }

    /// 调整窗口尺寸。
    pub fn resize(&self, rows: u16, cols: u16) {
        let _ = self.control.send(PtyControl::Resize { rows, cols });
    }

    /// 收一块 PTY 输出。`None` 表示 PTY 已结束。
    pub async fn next_output(&mut self) -> Option<Vec<u8>> {
        self.output.recv().await
    }

    /// 主动结束底层进程。
    pub fn shutdown(&self) {
        let _ = self.control.send(PtyControl::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_rejects_zero() {
        // 0 会让内核的 openpty/resize 报错，必须抬到至少 1。
        assert_eq!(clamp_size(0, 0), (1, 1));
    }

    #[test]
    fn clamp_caps_huge_values() {
        // 客户端发来的巨大尺寸（可能是恶意的）被封顶，避免异常分配。
        assert_eq!(clamp_size(9999, 9999), (1000, 1000));
    }

    #[test]
    fn clamp_keeps_normal_values() {
        assert_eq!(clamp_size(DEFAULT_ROWS, DEFAULT_COLS), (24, 80));
    }

    /// PTY 冒烟测试：spawn 一个确定性命令（/bin/echo），验证能从 PTY 读回它的输出。
    ///
    /// 用 echo 而非交互 shell，是为了让结果**确定、可断言**。默认跑；若 CI 环境
    /// 没有 /bin/echo 或 PTY 不可用，会读不到预期输出而失败——真出现这种环境
    /// 再给它加 #[ignore]。Windows 上路径不同，故仅在类 unix 跑。
    #[cfg(unix)]
    #[tokio::test]
    async fn echo_roundtrip_through_pty() {
        let cfg = PtyConfig::program("/bin/echo", vec!["hello-bastion".to_string()]);
        let mut bridge = PtyBridge::spawn(&cfg, DEFAULT_ROWS, DEFAULT_COLS).expect("spawn pty");

        // 累积输出直到看见我们 echo 的内容，或 PTY 结束。
        let mut acc = String::new();
        let found = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(chunk) = bridge.next_output().await {
                acc.push_str(&String::from_utf8_lossy(&chunk));
                if acc.contains("hello-bastion") {
                    return true;
                }
            }
            acc.contains("hello-bastion")
        })
        .await
        .expect("PTY 读取不应超时");

        assert!(found, "应从 PTY 读回 echo 的输出，实际收到：{acc:?}");
    }
}
