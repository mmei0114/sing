mod api;
mod config;
mod model;
mod native;
mod ruleset;
mod runtime;
mod subscription;
mod system_proxy;
mod ui;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "A terminal client for sing-box. Run without arguments to open the TUI."
)]
struct Args {
    #[arg(long, help = "Private application data directory")]
    data_dir: Option<PathBuf>,
    #[arg(long, hide = true)]
    daemon: bool,
    #[arg(long, hide = true)]
    tun_helper: bool,
    #[arg(long, hide = true)]
    system_proxy_helper: bool,
    #[arg(
        long,
        help = "Recover original macOS proxy settings (may request sudo)"
    )]
    restore_system_proxy: bool,
    #[arg(long, hide = true)]
    core: Option<PathBuf>,
    #[arg(
        long,
        help = "Use isolated sample data; never connects to a real proxy"
    )]
    demo: bool,
    #[arg(long, help = "Print a read-only JSON status snapshot")]
    status: bool,
    #[arg(long, help = "Disconnect the instance managed by this application")]
    disconnect: bool,
    #[arg(
        long,
        help = "Disconnect and stop this application's background manager"
    )]
    shutdown: bool,
    #[arg(long, help = "Render a static terminal preview to stdout")]
    preview: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let dir = args.data_dir.unwrap_or_else(model::default_dir);
    if args.system_proxy_helper {
        return system_proxy::helper::run(&dir);
    }
    if args.restore_system_proxy {
        if system_proxy::helper::query(&dir, system_proxy::helper::Request::Status).is_err() {
            eprintln!("Restore original macOS proxy settings. sudo will request authorization; no core will be started.");
            system_proxy::helper::authorize(&dir)?;
        }
        let s = system_proxy::helper::query(&dir, system_proxy::helper::Request::Restore)?;
        anyhow::ensure!(s.safe_to_stop, "{}", s.detail);
        system_proxy::helper::restore(&dir)?;
        println!("{}", s.detail);
        return Ok(());
    }
    if args.tun_helper {
        return runtime::tun_helper(
            &dir,
            args.core
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Missing --core"))?,
        );
    }
    if args.daemon {
        return runtime::daemon(&dir);
    }
    if args.preview {
        return ui::preview();
    }
    if args.status || args.disconnect || args.shutdown {
        let response = runtime::request(
            &dir,
            if args.shutdown {
                runtime::Action::Shutdown
            } else if args.disconnect {
                runtime::Action::Disconnect
            } else {
                runtime::Action::Snapshot
            },
        )?;
        println!("{}", serde_json::to_string_pretty(&response)?);
        anyhow::ensure!(response.ok, "{}", response.message);
        return Ok(());
    }
    ui::run(dir, args.demo)
}
