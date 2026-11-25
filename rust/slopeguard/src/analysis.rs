//! Leakage-safe feature construction, independent labels, and transparent baselines.
//!
//! CSV is intentionally small and dependency-free. It is an exchange artifact,
//! not a second data model: raw telemetry remains authoritative in SQLite.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use crate::ingest::Store;
use chrono::{DateTime, Duration, FixedOffset};

pub const HORIZON_MINUTES: i64 = 30;
const WINDOW_15_MINUTES: i64 = 15;
const REFERENCE_STALE_AFTER_SECONDS: i64 = 60;
type NodeKey = (String, String);
type PreviousFeatures = (DateTime<FixedOffset>, Option<f64>, Option<f64>);
type EventBounds = (DateTime<FixedOffset>, DateTime<FixedOffset>);
type EventTimes = BTreeMap<String, EventBounds>;

pub const FEATURE_HEADER: &str = "site_id,node_id,ts_device,pitch_deg,roll_deg,relative_pitch_deg,relative_roll_deg,tilt_rate_deg_min,accel_mag_g,vibration_mag_g,rain_15m_mm,rain_30m_mm,soil_moisture_pct,soil_rate_pct_min,missing_count,stale_count,reference_age_s,environment_missing,movement_missing,reference_missing";
pub const LABEL_EXTRA_HEADER: &str = "event_id,movement_event,label_status,evidence_source,displacement_threshold_mm,horizon_minutes";
pub const EVENT_HEADER: &str = "event_id,start_ts,end_ts,scenario";
pub const GROUND_TRUTH_HEADER: &str =
    "event_id,ts_device,displacement_mm,evidence_file,review_status";
pub const SPLIT_HEADER: &str = "event_id,split,start_ts,end_ts,row_count";
pub const BASELINE_HEADER: &str = "baseline,event_id,split,ts_device,actual,predicted,score,reason";

#[derive(Debug, Clone, PartialEq)]
pub struct FeatureRow {
    pub site_id: String,
    pub node_id: String,
    pub ts_device: String,
    pub pitch_deg: f64,
    pub roll_deg: f64,
    pub relative_pitch_deg: Option<f64>,
    pub relative_roll_deg: Option<f64>,
    pub tilt_rate_deg_min: Option<f64>,
    pub accel_mag_g: Option<f64>,
    pub vibration_mag_g: Option<f64>,
    pub rain_15m_mm: Option<f64>,
    pub rain_30m_mm: Option<f64>,
    pub soil_moisture_pct: Option<f64>,
    pub soil_rate_pct_min: Option<f64>,
    pub missing_count: u32,
    pub stale_count: u32,
    pub reference_age_s: Option<i64>,
    pub environment_missing: bool,
    pub movement_missing: bool,
    pub reference_missing: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct PacketRow {
    site_id: String,
    node_id: String,
    role: String,
    ts: DateTime<FixedOffset>,
    pitch: f64,
    roll: f64,
    accel_mag: f64,
    rain: Option<f64>,
    soil: Option<f64>,
    sensor_status: String,
}

fn parse_ts(value: &str) -> Result<DateTime<FixedOffset>, String> {
    DateTime::parse_from_rfc3339(value).map_err(|e| format!("bad timestamp {value}: {e}"))
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn opt(value: Option<f64>) -> String {
    value.map_or_else(String::new, |x| format!("{x:.6}"))
}

fn opt_i64(value: Option<i64>) -> String {
    value.map_or_else(String::new, |x| x.to_string())
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut count = 0_u32;
    let mut total = 0.0;
    for value in values {
        total += value;
        count += 1;
    }
    (count > 0).then_some(total / f64::from(count))
}

fn slope_per_minute(
    previous: Option<(DateTime<FixedOffset>, f64)>,
    current: (DateTime<FixedOffset>, f64),
) -> Option<f64> {
    let (previous_ts, previous_value) = previous?;
    let minutes = (current.0 - previous_ts).num_milliseconds() as f64 / 60_000.0;
    (minutes > 0.0).then_some((current.1 - previous_value) / minutes)
}

fn packet_rows(store: &Store) -> Result<Vec<PacketRow>, Box<dyn std::error::Error>> {
    let mut stmt = store.conn.prepare(
        "SELECT site_id, node_id, node_role, ts_device, payload, sensor_status
         FROM raw_packet ORDER BY ts_device, id",
    )?;
    let rows = stmt.query_map([], |row| {
        let payload: String = row.get(4)?;
        let packet: serde_json::Value = serde_json::from_str(&payload).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                payload.len(),
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?;
        let accel = ["accel_x_g", "accel_y_g", "accel_z_g"]
            .into_iter()
            .map(|key| packet[key].as_f64().unwrap_or(f64::NAN))
            .collect::<Vec<_>>();
        Ok(PacketRow {
            site_id: row.get(0)?,
            node_id: row.get(1)?,
            role: row.get(2)?,
            ts: parse_ts(packet["ts_device"].as_str().unwrap_or_default()).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
            })?,
            pitch: packet["tilt_pitch_deg"].as_f64().unwrap_or(f64::NAN),
            roll: packet["tilt_roll_deg"].as_f64().unwrap_or(f64::NAN),
            accel_mag: accel.iter().map(|value| value * value).sum::<f64>().sqrt(),
            rain: packet["rain_mm_interval"].as_f64(),
            soil: packet["soil_moisture_pct"].as_f64(),
            sensor_status: row.get(5)?,
        })
    })?;
    let mut rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.sort_by_key(|row| row.ts);
    Ok(rows)
}

/// Build one row per movement-node timestamp. Every aggregate uses packets with
/// `ts_device <= prediction timestamp`; the reference is as-of joined, never
/// taken from the future. Missing values remain missing and are counted.
pub fn build_features(store: &Store) -> Result<Vec<FeatureRow>, Box<dyn std::error::Error>> {
    let packets = packet_rows(store)?;
    let movement: Vec<&PacketRow> = packets.iter().filter(|p| p.role == "movement").collect();
    let references: Vec<&PacketRow> = packets.iter().filter(|p| p.role == "reference").collect();
    let mut output = Vec::with_capacity(movement.len());
    let mut previous_movement: HashMap<NodeKey, PreviousFeatures> = HashMap::new();

    for current in movement {
        let key = (current.site_id.clone(), current.node_id.clone());
        let reference = references
            .iter()
            .filter(|reference| reference.site_id == current.site_id && reference.ts <= current.ts)
            .max_by_key(|reference| reference.ts);
        let movement_history: Vec<&PacketRow> = packets
            .iter()
            .filter(|packet| {
                packet.site_id == current.site_id
                    && packet.node_id == current.node_id
                    && packet.role == "movement"
                    && packet.ts <= current.ts
                    && packet.ts >= current.ts - Duration::minutes(WINDOW_15_MINUTES)
            })
            .collect();
        let rain_15m_mm = if movement_history.iter().any(|packet| packet.rain.is_some()) {
            Some(
                movement_history
                    .iter()
                    .filter_map(|packet| packet.rain)
                    .sum(),
            )
        } else {
            None
        };
        let rain_30m_packets: Vec<&PacketRow> = packets
            .iter()
            .filter(|packet| {
                packet.site_id == current.site_id
                    && packet.node_id == current.node_id
                    && packet.role == "movement"
                    && packet.ts <= current.ts
                    && packet.ts >= current.ts - Duration::minutes(30)
            })
            .collect();
        let rain_30m_mm = if rain_30m_packets.iter().any(|packet| packet.rain.is_some()) {
            Some(
                rain_30m_packets
                    .iter()
                    .filter_map(|packet| packet.rain)
                    .sum(),
            )
        } else {
            None
        };
        let soil_moisture_pct = current.soil;
        let previous = previous_movement.get(&key).copied();
        let soil_rate_pct_min = previous.and_then(|(previous_ts, previous_soil, _)| {
            slope_per_minute(
                previous_soil.map(|soil| (previous_ts, soil)),
                (current.ts, soil_moisture_pct?),
            )
        });
        let reference_age_s = reference.map(|reference| (current.ts - reference.ts).num_seconds());
        let reference_fresh = reference.is_some_and(|reference| {
            reference.sensor_status == "OK"
                && reference_age_s.is_some_and(|age| age <= REFERENCE_STALE_AFTER_SECONDS)
        });
        let relative_pitch_deg = reference_fresh
            .then(|| reference.map(|reference| current.pitch - reference.pitch))
            .flatten();
        let relative_roll_deg = reference_fresh
            .then(|| reference.map(|reference| current.roll - reference.roll))
            .flatten();
        let tilt_rate_deg_min = previous.and_then(|(previous_ts, _, previous_relative)| {
            slope_per_minute(
                previous_relative.map(|value| (previous_ts, value)),
                (current.ts, relative_pitch_deg?),
            )
        });
        let prior_accel: Vec<f64> = movement_history
            .iter()
            .map(|packet| packet.accel_mag)
            .collect();
        let accel_mag_g = Some(current.accel_mag);
        let vibration_mag_g = mean(prior_accel.into_iter().map(|value| (value - 1.0).abs()));
        let environment_missing = current.rain.is_none() || current.soil.is_none();
        let movement_missing = current.sensor_status != "OK";
        let reference_missing = !reference_fresh;
        let missing_count = [
            relative_pitch_deg.is_none(),
            relative_roll_deg.is_none(),
            tilt_rate_deg_min.is_none(),
            rain_15m_mm.is_none(),
            rain_30m_mm.is_none(),
            soil_moisture_pct.is_none(),
            soil_rate_pct_min.is_none(),
            reference_missing,
        ]
        .into_iter()
        .filter(|missing| *missing)
        .count() as u32;
        let stale_count = u32::from(
            reference.is_some_and(|packet| packet.sensor_status != "OK")
                || reference_age_s.is_some_and(|age| age > REFERENCE_STALE_AFTER_SECONDS),
        );
        output.push(FeatureRow {
            site_id: current.site_id.clone(),
            node_id: current.node_id.clone(),
            ts_device: current.ts.to_rfc3339(),
            pitch_deg: current.pitch,
            roll_deg: current.roll,
            relative_pitch_deg,
            relative_roll_deg,
            tilt_rate_deg_min,
            accel_mag_g,
            vibration_mag_g,
            rain_15m_mm,
            rain_30m_mm,
            soil_moisture_pct,
            soil_rate_pct_min,
            missing_count,
            stale_count,
            reference_age_s,
            environment_missing,
            movement_missing,
            reference_missing,
        });
        previous_movement.insert(
            (key.0, key.1),
            (current.ts, current.soil, relative_pitch_deg),
        );
    }
    Ok(output)
}

pub fn write_features_csv(rows: &[FeatureRow], path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    writeln!(out, "{FEATURE_HEADER}").expect("writing String cannot fail");
    for row in rows {
        writeln!(
            out,
            "{},{},{},{:.6},{:.6},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            csv_escape(&row.site_id),
            csv_escape(&row.node_id),
            csv_escape(&row.ts_device),
            row.pitch_deg,
            row.roll_deg,
            opt(row.relative_pitch_deg),
            opt(row.relative_roll_deg),
            opt(row.tilt_rate_deg_min),
            opt(row.accel_mag_g),
            opt(row.vibration_mag_g),
            opt(row.rain_15m_mm),
            opt(row.rain_30m_mm),
            opt(row.soil_moisture_pct),
            opt(row.soil_rate_pct_min),
            row.missing_count,
            row.stale_count,
            opt_i64(row.reference_age_s),
            bool_text(row.environment_missing),
            bool_text(row.movement_missing),
            bool_text(row.reference_missing),
        )
        .expect("writing String cannot fail");
    }
    std::fs::write(path, out)
}

#[derive(Debug, Clone)]
struct Event {
    id: String,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
}

#[derive(Debug, Clone)]
struct TruthPoint {
    event_id: String,
    ts: DateTime<FixedOffset>,
    displacement_mm: f64,
    evidence: String,
    reviewed: bool,
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

fn read_csv(path: &Path) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(split_csv_line)
        .collect())
}

fn parse_events(path: &Path) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let mut rows = read_csv(path)?.into_iter();
    let header = rows.next().ok_or("empty events csv")?;
    if header != split_csv_line(EVENT_HEADER) {
        return Err("events header does not match EVENT_HEADER".into());
    }
    let mut events = Vec::new();
    for row in rows {
        if row.len() < 3 {
            return Err("event row has fewer than 3 fields".into());
        }
        let event = Event {
            id: row[0].clone(),
            start: parse_ts(&row[1])?,
            end: parse_ts(&row[2])?,
        };
        if event.end <= event.start {
            return Err("event end must be after event start".into());
        }
        events.push(event);
    }
    Ok(events)
}

fn parse_truth(path: &Path) -> Result<Vec<TruthPoint>, Box<dyn std::error::Error>> {
    let mut rows = read_csv(path)?.into_iter();
    let header = rows.next().ok_or("empty ground truth csv")?;
    if header != split_csv_line(GROUND_TRUTH_HEADER) {
        return Err("ground truth header does not match GROUND_TRUTH_HEADER".into());
    }
    rows.map(|row| {
        if row.len() != 5 {
            return Err("ground truth row has fewer than 5 fields".into());
        }
        Ok(TruthPoint {
            event_id: row[0].clone(),
            ts: parse_ts(&row[1])?,
            displacement_mm: row[2].parse()?,
            evidence: row[3].clone(),
            reviewed: row[4] == "reviewed",
        })
    })
    .collect()
}

/// Generate labels from independent displacement points. The current point is
/// excluded from the future interval: labels inspect `(t, t + horizon]` only.
pub fn generate_labels(
    features_path: &Path,
    events_path: &Path,
    truth_path: &Path,
    displacement_threshold_mm: f64,
    out_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let feature_rows = read_csv(features_path)?;
    let header = feature_rows.first().ok_or("empty features csv")?.clone();
    if header != split_csv_line(FEATURE_HEADER) {
        return Err("features header does not match FEATURE_HEADER".into());
    }
    let timestamp_index = header
        .iter()
        .position(|v| v == "ts_device")
        .ok_or("features missing ts_device")?;
    let events = parse_events(events_path)?;
    let truth = parse_truth(truth_path)?;
    let mut out = String::new();
    writeln!(out, "{FEATURE_HEADER},{LABEL_EXTRA_HEADER}").expect("writing String cannot fail");
    let mut count = 0;
    for row in feature_rows.into_iter().skip(1) {
        if row.len() != header.len() {
            return Err("feature row width does not match FEATURE_HEADER".into());
        }
        let ts = parse_ts(&row[timestamp_index])?;
        let matching_events: Vec<&Event> = events
            .iter()
            .filter(|event| ts >= event.start && ts <= event.end)
            .collect();
        let event = match matching_events.as_slice() {
            [event] => *event,
            [] => return Err(format!("feature timestamp is outside events: {ts}").into()),
            _ => return Err(format!("overlapping events at feature timestamp: {ts}").into()),
        };
        let event_id = event.id.clone();
        let horizon_end = ts + Duration::minutes(HORIZON_MINUTES);
        let in_event = truth
            .iter()
            .filter(|point| {
                point.event_id == event_id
                    && point.reviewed
                    && point.ts > ts
                    && point.ts <= horizon_end
                    && point.ts >= event.start
                    && point.ts <= event.end
            })
            .collect::<Vec<_>>();
        let status = if horizon_end > event.end {
            "INSUFFICIENT_HORIZON"
        } else if in_event.is_empty() {
            "NO_INDEPENDENT_EVIDENCE"
        } else {
            "LABELED"
        };
        let positive = in_event
            .iter()
            .any(|point| point.displacement_mm >= displacement_threshold_mm);
        let source = in_event
            .first()
            .map_or(String::new(), |point| point.evidence.clone());
        let label = if status == "LABELED" {
            i32::from(positive)
        } else {
            -1
        };
        writeln!(
            out,
            "{},{},{},{},{},{},{}",
            row.join(","),
            event_id,
            label,
            status,
            csv_escape(&source),
            displacement_threshold_mm,
            HORIZON_MINUTES
        )
        .expect("writing String cannot fail");
        count += 1;
    }
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, out)?;
    Ok(count)
}

#[derive(Debug, Clone, Copy)]
pub struct BaselineConfig {
    pub rain_15m_mm: f64,
    pub soil_moisture_pct: f64,
    pub tilt_rate_deg_min: f64,
    pub relative_pitch_deg: f64,
}

impl Default for BaselineConfig {
    fn default() -> Self {
        Self {
            rain_15m_mm: 5.0,
            soil_moisture_pct: 75.0,
            tilt_rate_deg_min: 0.2,
            relative_pitch_deg: 2.0,
        }
    }
}

#[derive(Debug, Clone)]
struct LabelRow {
    event_id: String,
    ts: String,
    actual: i32,
    status: String,
    rain_15m: Option<f64>,
    soil: Option<f64>,
    tilt_rate: Option<f64>,
    relative_pitch: Option<f64>,
}

fn column(header: &[String], name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    header
        .iter()
        .position(|v| v == name)
        .ok_or_else(|| format!("missing column {name}").into())
}

fn parse_label_rows(path: &Path) -> Result<Vec<LabelRow>, Box<dyn std::error::Error>> {
    let rows = read_csv(path)?;
    let header = rows.first().ok_or("empty labels csv")?.clone();
    let expected_header = split_csv_line(&format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}"));
    if header != expected_header {
        return Err("labels header does not match feature and label contract".into());
    }
    let event_idx = column(&header, "event_id")?;
    let ts_idx = column(&header, "ts_device")?;
    let actual_idx = column(&header, "movement_event")?;
    let status_idx = column(&header, "label_status")?;
    let rain_idx = column(&header, "rain_15m_mm")?;
    let soil_idx = column(&header, "soil_moisture_pct")?;
    let rate_idx = column(&header, "tilt_rate_deg_min")?;
    let relative_idx = column(&header, "relative_pitch_deg")?;
    rows.into_iter()
        .skip(1)
        .map(|row| {
            if row.len() != header.len() {
                return Err("label row width does not match labels header".into());
            }
            Ok(LabelRow {
                event_id: row[event_idx].clone(),
                ts: row[ts_idx].clone(),
                actual: row[actual_idx].parse()?,
                status: row[status_idx].clone(),
                rain_15m: row[rain_idx].parse().ok(),
                soil: row[soil_idx].parse().ok(),
                tilt_rate: row[rate_idx].parse().ok(),
                relative_pitch: row[relative_idx].parse().ok(),
            })
        })
        .collect()
}

fn event_times(rows: &[LabelRow]) -> Result<EventTimes, Box<dyn std::error::Error>> {
    let mut map: EventTimes = BTreeMap::new();
    for row in rows {
        let ts = parse_ts(&row.ts)?;
        let entry = map.entry(row.event_id.clone()).or_insert((ts, ts));
        if ts < entry.0 {
            entry.0 = ts;
        }
        if ts > entry.1 {
            entry.1 = ts;
        }
    }
    Ok(map)
}

/// Assign complete events chronologically: earliest 60% train, next 20%
/// validation, latest 20% test. An event cannot cross split boundaries.
pub fn write_split_manifest(
    labels_path: &Path,
    out_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let rows = parse_label_rows(labels_path)?;
    let times = event_times(&rows)?;
    if times.len() < 3 {
        return Err("at least 3 events are required for train/validation/test".into());
    }
    let mut ids: Vec<String> = times.keys().cloned().collect();
    ids.sort_by(|left, right| {
        times[left]
            .0
            .cmp(&times[right].0)
            .then_with(|| left.cmp(right))
    });
    let n = ids.len();
    let train_end = (n * 3).div_ceil(5).clamp(1, n - 2);
    let validation_end = (n * 4).div_ceil(5).clamp(train_end + 1, n - 1);
    let split_for = |index: usize| {
        if index < train_end {
            "train"
        } else if index < validation_end {
            "validation"
        } else {
            "test"
        }
    };
    let mut out = String::new();
    writeln!(out, "{SPLIT_HEADER}").expect("writing String cannot fail");
    for (index, id) in ids.iter().enumerate() {
        let (start, end) = &times[id];
        let row_count = rows.iter().filter(|row| row.event_id == *id).count();
        writeln!(
            out,
            "{id},{},{},{},{row_count}",
            split_for(index),
            start.to_rfc3339(),
            end.to_rfc3339()
        )
        .expect("writing String cannot fail");
    }
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, out)?;
    Ok(ids.len())
}

fn split_map(path: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let rows = read_csv(path)?;
    let mut map = HashMap::new();
    for row in rows.into_iter().skip(1) {
        if row.len() < 2 {
            return Err("split row has fewer than 2 fields".into());
        }
        map.insert(row[0].clone(), row[1].clone());
    }
    Ok(map)
}

fn b0(row: &LabelRow, config: BaselineConfig) -> (i32, f64, &'static str) {
    let rain = row.rain_15m.unwrap_or(0.0);
    let soil = row.soil.unwrap_or(0.0);
    let score = (rain / config.rain_15m_mm).max(soil / config.soil_moisture_pct);
    (
        i32::from(rain >= config.rain_15m_mm && soil >= config.soil_moisture_pct),
        score,
        "rain_and_soil",
    )
}

fn b1(row: &LabelRow, config: BaselineConfig) -> (i32, f64, &'static str) {
    let rate = row.tilt_rate.unwrap_or(0.0).abs();
    (
        i32::from(rate >= config.tilt_rate_deg_min),
        rate / config.tilt_rate_deg_min,
        "tilt_rate",
    )
}

fn b2(row: &LabelRow, config: BaselineConfig) -> (i32, f64, &'static str) {
    let environmental = row.rain_15m.is_some_and(|rain| rain >= config.rain_15m_mm)
        && row
            .soil
            .is_some_and(|soil| soil >= config.soil_moisture_pct);
    let motion = row
        .tilt_rate
        .is_some_and(|rate| rate.abs() >= config.tilt_rate_deg_min)
        || row
            .relative_pitch
            .is_some_and(|pitch| pitch.abs() >= config.relative_pitch_deg);
    (
        i32::from(environmental && motion),
        f64::from(environmental) + f64::from(motion),
        "environment_and_motion",
    )
}

pub fn write_baseline_results(
    labels_path: &Path,
    split_path: &Path,
    out_path: &Path,
    config: BaselineConfig,
) -> Result<usize, Box<dyn std::error::Error>> {
    let rows = parse_label_rows(labels_path)?;
    let splits = split_map(split_path)?;
    let mut out = String::new();
    writeln!(out, "{BASELINE_HEADER}").expect("writing String cannot fail");
    let mut count = 0;
    for row in rows {
        if row.status != "LABELED" {
            continue;
        }
        let Some(split) = splits.get(&row.event_id) else {
            return Err(format!("missing split for {}", row.event_id).into());
        };
        for (name, (predicted, score, reason)) in [
            ("B0", b0(&row, config)),
            ("B1", b1(&row, config)),
            ("B2", b2(&row, config)),
        ] {
            writeln!(
                out,
                "{name},{},{split},{},{},{predicted},{score:.6},{reason}",
                row.event_id, row.ts, row.actual
            )
            .expect("writing String cannot fail");
            count += 1;
        }
    }
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, out)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::{csv_escape, split_csv_line};

    #[test]
    fn csv_roundtrip_quotes() {
        let value = "camera,marker \"review\"";
        assert_eq!(split_csv_line(&csv_escape(value)), vec![value]);
    }
}
