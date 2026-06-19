use std::io::Write;
use std::time::Instant;

pub struct PhaseTrace {
    label: &'static str,
    start: Option<Instant>,
    start_rss: Option<u64>,
    enabled: bool,
    rss_enabled: bool,
}

impl PhaseTrace {
    pub fn new(label: &'static str) -> Self {
        let enabled = phase_trace_enabled();
        let rss_enabled = enabled && phase_trace_rss_enabled();
        Self {
            label,
            start: enabled.then(Instant::now),
            start_rss: rss_enabled.then(current_rss_bytes).flatten(),
            enabled,
            rss_enabled,
        }
    }
}

impl Drop for PhaseTrace {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        let Some(start) = self.start else {
            return;
        };
        let elapsed = start.elapsed().as_secs_f64();
        let end_rss = self.rss_enabled.then(current_rss_bytes).flatten();
        let delta = match (self.start_rss, end_rss) {
            (Some(start), Some(end)) => end as i64 - start as i64,
            _ => 0,
        };
        let line = format!(
            "zmin-phase\t{}\tseconds={elapsed:.6}\trss_bytes={}\trss_delta_bytes={delta}",
            self.label,
            end_rss.unwrap_or(0)
        );
        write_phase_trace_line(&line);
    }
}

pub fn phase_trace(label: &'static str) -> PhaseTrace {
    PhaseTrace::new(label)
}

pub fn phase_trace_emit(label: &'static str, seconds: f64, fields: &[(&str, String)]) {
    if !phase_trace_enabled() {
        return;
    }
    let mut line = format!("zmin-phase\t{label}\tseconds={seconds:.6}");
    for (key, value) in fields {
        line.push('\t');
        line.push_str(key);
        line.push('=');
        line.push_str(value);
    }
    write_phase_trace_line(&line);
}

pub fn phase_trace_enabled() -> bool {
    std::env::var_os("ZMIN_PHASE_TRACE").is_some_and(|value| !value.is_empty())
}

fn phase_trace_rss_enabled() -> bool {
    std::env::var_os("ZMIN_PHASE_TRACE_RSS").is_some_and(|value| !value.is_empty())
}

fn write_phase_trace_line(line: &str) {
    if let Some(path) = std::env::var_os("ZMIN_PHASE_TRACE_FILE") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
        }
    } else {
        eprintln!("{line}");
    }
}

#[cfg(target_os = "linux")]
fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(pages.saturating_mul(4096))
}

#[cfg(target_os = "macos")]
fn current_rss_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rss_kib = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(rss_kib.saturating_mul(1024))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_rss_bytes() -> Option<u64> {
    None
}
