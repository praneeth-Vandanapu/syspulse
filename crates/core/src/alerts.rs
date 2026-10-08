// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/alerts.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Threshold alerts evaluated on every telemetry sample: CPU / GPU
//           temperature, memory pressure, nearly full filesystems and low
//           battery. Conditions must persist for a while before firing
//           (no flapping), each alert has a cooldown, and an alert re-arms
//           only after the value drops back below a hysteresis margin.
// =============================================================================

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::models::Telemetry;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlertConfig {
    pub enabled: bool,
    /// °C
    pub cpu_temp: f32,
    /// °C
    pub gpu_temp: f32,
    /// percent of RAM in use
    pub memory_percent: f32,
    /// percent of a filesystem in use
    pub disk_percent: f32,
    /// percent remaining while discharging
    pub battery_percent: f32,
    /// How long a temperature / memory condition must last before alerting.
    pub sustain_secs: u64,
    /// Minimum time between two alerts of the same kind.
    pub cooldown_secs: u64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        AlertConfig {
            enabled: true,
            cpu_temp: 90.0,
            gpu_temp: 85.0,
            memory_percent: 95.0,
            disk_percent: 95.0,
            battery_percent: 15.0,
            sustain_secs: 30,
            cooldown_secs: 900,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub key: String,
    pub title: String,
    pub body: String,
    /// warning | critical
    pub level: &'static str,
}

#[derive(Default)]
struct State {
    since: Option<Instant>,
    last_fired: Option<Instant>,
    armed: bool,
}

pub struct AlertEngine {
    pub config: AlertConfig,
    states: HashMap<String, State>,
}

/// How far below the threshold a value must fall before the alert re-arms.
const HYSTERESIS: f32 = 3.0;

impl AlertEngine {
    pub fn new(config: AlertConfig) -> Self {
        AlertEngine {
            config,
            states: HashMap::new(),
        }
    }

    pub fn check(&mut self, t: &Telemetry) -> Vec<Alert> {
        self.check_at(t, Instant::now())
    }

    /// Evaluate one sample at time `now` (separate for testing).
    pub fn check_at(&mut self, t: &Telemetry, now: Instant) -> Vec<Alert> {
        if !self.config.enabled {
            return Vec::new();
        }
        let c = self.config.clone();
        let sustain = Duration::from_secs(c.sustain_secs);
        let mut out = Vec::new();

        if let Some(temp) = t.cpu.temperature {
            self.eval(&mut out, now, "cpu-temp", temp, c.cpu_temp, sustain, || Alert {
                key: "cpu-temp".into(),
                title: "CPU is running hot".into(),
                body: format!("CPU temperature is {temp:.0} °C (alert at {:.0} °C). Check cooling and busy processes.", c.cpu_temp),
                level: if temp >= c.cpu_temp + 5.0 { "critical" } else { "warning" },
            });
        }
        for g in &t.gpus {
            if let Some(temp) = g.temperature {
                let key = format!("gpu-temp:{}", g.id);
                self.eval(&mut out, now, &key, temp, c.gpu_temp, sustain, || Alert {
                    key: key.clone(),
                    title: "GPU is running hot".into(),
                    body: format!(
                        "{} is at {temp:.0} °C (alert at {:.0} °C).",
                        g.name, c.gpu_temp
                    ),
                    level: "warning",
                });
            }
        }
        if t.memory.total > 0 {
            let used = t.memory.used as f32 / t.memory.total as f32 * 100.0;
            self.eval(&mut out, now, "memory", used, c.memory_percent, sustain, || Alert {
                key: "memory".into(),
                title: "Memory almost full".into(),
                body: format!("{used:.0}% of RAM is in use. Close unused applications to avoid slowdowns."),
                level: "warning",
            });
        }
        for f in &t.filesystems {
            if f.total == 0 || f.removable {
                continue;
            }
            let used = (f.total - f.available) as f32 / f.total as f32 * 100.0;
            let key = format!("disk:{}", f.mount_point);
            let free = crate::util::human_bytes(f.available);
            self.eval(
                &mut out,
                now,
                &key,
                used,
                c.disk_percent,
                Duration::ZERO,
                || Alert {
                    key: key.clone(),
                    title: format!("Disk {} is almost full", f.mount_point),
                    body: format!(
                        "{used:.0}% used, only {free} free. Try Cache Cleaner or Large Files."
                    ),
                    level: if used >= 98.0 { "critical" } else { "warning" },
                },
            );
        }
        if let Some(b) = &t.battery {
            let discharging = b.status.eq_ignore_ascii_case("discharging");
            // Low battery = value below threshold, so evaluate the inverted value.
            let deficit = if discharging { 100.0 - b.percent } else { 0.0 };
            self.eval(
                &mut out,
                now,
                "battery",
                deficit,
                100.0 - c.battery_percent,
                Duration::ZERO,
                || Alert {
                    key: "battery".into(),
                    title: "Battery low".into(),
                    body: format!("Battery at {:.0}%. Connect the charger.", b.percent),
                    level: if b.percent <= 5.0 {
                        "critical"
                    } else {
                        "warning"
                    },
                },
            );
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn eval(
        &mut self,
        out: &mut Vec<Alert>,
        now: Instant,
        key: &str,
        value: f32,
        threshold: f32,
        sustain: Duration,
        make: impl FnOnce() -> Alert,
    ) {
        let cooldown = Duration::from_secs(self.config.cooldown_secs);
        let st = self.states.entry(key.to_string()).or_insert(State {
            armed: true,
            ..Default::default()
        });
        if value >= threshold {
            let since = *st.since.get_or_insert(now);
            let cooled = st
                .last_fired
                .is_none_or(|t| now.duration_since(t) >= cooldown);
            if st.armed && cooled && now.duration_since(since) >= sustain {
                st.last_fired = Some(now);
                st.armed = false;
                out.push(make());
            }
        } else {
            st.since = None;
            if value < threshold - HYSTERESIS {
                st.armed = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn sample(cpu_temp: f32, disk_used_pct: f64) -> Telemetry {
        Telemetry {
            timestamp: 0,
            cpu: CpuSample {
                temperature: Some(cpu_temp),
                ..Default::default()
            },
            memory: MemorySample {
                total: 100,
                used: 10,
                ..Default::default()
            },
            filesystems: vec![FilesystemSample {
                device: "/dev/x".into(),
                mount_point: "/".into(),
                fs_type: "ext4".into(),
                total: 1000,
                available: (1000.0 * (1.0 - disk_used_pct / 100.0)) as u64,
                removable: false,
            }],
            disk_io: vec![],
            network: vec![],
            sensors: vec![],
            battery: None,
            gpus: vec![],
        }
    }

    #[test]
    fn sustained_temperature_fires_once() {
        let mut e = AlertEngine::new(AlertConfig {
            sustain_secs: 10,
            ..Default::default()
        });
        let t0 = Instant::now();
        assert!(
            e.check_at(&sample(95.0, 10.0), t0).is_empty(),
            "not sustained yet"
        );
        let a = e.check_at(&sample(95.0, 10.0), t0 + Duration::from_secs(11));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].key, "cpu-temp");
        assert!(
            e.check_at(&sample(96.0, 10.0), t0 + Duration::from_secs(20))
                .is_empty(),
            "no repeat while hot"
        );
        // Cool down below hysteresis, then heat again after the cooldown.
        e.check_at(&sample(60.0, 10.0), t0 + Duration::from_secs(30));
        let later = t0 + Duration::from_secs(2000);
        e.check_at(&sample(95.0, 10.0), later);
        assert_eq!(
            e.check_at(&sample(95.0, 10.0), later + Duration::from_secs(11))
                .len(),
            1
        );
    }

    #[test]
    fn disk_fires_immediately_and_disabled_is_silent() {
        let mut e = AlertEngine::new(AlertConfig::default());
        let a = e.check_at(&sample(40.0, 97.0), Instant::now());
        assert_eq!(a.len(), 1);
        assert!(a[0].title.contains("almost full"));
        let mut off = AlertEngine::new(AlertConfig {
            enabled: false,
            ..Default::default()
        });
        assert!(off.check_at(&sample(99.0, 99.0), Instant::now()).is_empty());
    }
}
