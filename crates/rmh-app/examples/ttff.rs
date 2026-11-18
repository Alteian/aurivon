//! Time-to-first-frame probe.
//!
//! `cargo run -p rmh-app --example ttff -- <imdb> [index|fast]`
//!
//! Resolves the stream at `index` of the search results (default 0, the top
//! row), or with `fast` runs "Play best" over the saved stream filter, then
//! times how long mpv takes to decode the first video frame.
use rmh_app::MediaService;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let imdb = args.next().unwrap_or_else(|| "tt1160419".into());
    let pick = args.next().unwrap_or_else(|| "0".into());
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let svc = MediaService::new()?;
    let t = Instant::now();
    let hits = block_on(svc.search_streams_for(imdb.clone(), None)).map_err(anyhow::Error::msg)?;
    println!("search: {} hits in {:.2}s", hits.len(), t.elapsed().as_secs_f64());
    let t0 = Instant::now();
    let r = if pick == "fast" {
        let order = rmh_app::streams::fast_start_order(&hits, &svc.config().stream_filter);
        for i in order.iter().take(5) {
            let h = &hits[*i];
            println!("  candidate #{i}: {:?} seeders {:?} {}", h.seeders, h.size, h.title);
        }
        block_on(svc.fast_start(order.into_iter().map(|i| hits[i].clone()).collect()))
    } else {
        // a number, or "hash:<prefix>" for one specific torrent
        let idx: usize = match pick.strip_prefix("hash:") {
            Some(h) => hits.iter().position(|x| x.info_hash.starts_with(h)).ok_or_else(|| anyhow::anyhow!("no hash {h}"))?,
            None => pick.parse()?,
        };
        let hit = hits.get(idx).ok_or_else(|| anyhow::anyhow!("no hit {idx}"))?;
        println!("pick #{idx}: {} | {:?} seeders | {:?}", hit.title, hit.seeders, hit.size);
        block_on(svc.resolve_stream(hit))
    }
    .map_err(anyhow::Error::msg)?;
    let resolved = t0.elapsed().as_secs_f64();
    println!("resolved in {resolved:.2}s via {} -> {}", if r.debrid { "Real-Debrid" } else { "torrent" }, r.hit_title);
    let st = std::process::Command::new("timeout")
        .args(["240", "mpv", "--no-config", "--vo=null", "--ao=null", "--frames=1", "--no-terminal", &r.url])
        .status()?;
    let total = t0.elapsed().as_secs_f64();
    println!("TTFF {total:.2}s (resolve {resolved:.2}s + mpv {:.2}s) ok={}", total - resolved, st.success());
    Ok(())
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}
