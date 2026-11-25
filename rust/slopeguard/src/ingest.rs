//! Ingestion: validate telemetry, persist raw packets unchanged, log rejects.
//!
//! Transport (HTTPS endpoint) is step 2; this module is the shared code path it
//! will call, and today's CLI replay entry point.
//!
//! ```text
//! slopeguard ingest data/replays/stable_dry.jsonl --db data/raw/slopeguard.db
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use serde_json::Value;

use crate::contract::validate;

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS raw_packet (
    id            INTEGER PRIMARY KEY,
    site_id       TEXT NOT NULL,
    node_id       TEXT NOT NULL,
    seq           INTEGER NOT NULL,
    node_role     TEXT NOT NULL,
    ts_device     TEXT NOT NULL,
    received_at   TEXT NOT NULL,
    sensor_status TEXT NOT NULL,
    battery_v     REAL,
    signal_dbm    REAL,
    payload       TEXT NOT NULL,
    UNIQUE (site_id, node_id, seq)
);
CREATE TABLE IF NOT EXISTS rejected (
    id          INTEGER PRIMARY KEY,
    received_at TEXT NOT NULL,
    source      TEXT NOT NULL,
    payload     TEXT NOT NULL,
    reason      TEXT NOT NULL
);
";

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// One storage path: SQLite. Raw payload text is never rewritten.
pub struct Store {
    pub conn: Connection,
}

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }
}

fn reject(conn: &mut Connection, source: &str, raw: &str, reason: &str) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO rejected (received_at, source, payload, reason) VALUES (?,?,?,?)",
        params![now(), source, raw, reason],
    )?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub accepted: u64,
    pub rejected: u64,
    pub duplicates: u64,
    pub out_of_order: u64,
    pub errors: BTreeMap<String, u64>,
}

impl Report {
    fn count(&mut self, code: &str) {
        *self.errors.entry(code.to_string()).or_insert(0) += 1;
    }
}

/// Ingest JSONL lines. Returns a report; bad data is logged, never raised.
pub fn ingest_lines<S: AsRef<str>>(
    store: &mut Store,
    lines: impl IntoIterator<Item = S>,
    source: &str,
) -> Result<Report> {
    let mut report = Report::default();

    for line in lines {
        let raw = line.as_ref().trim();
        if raw.is_empty() {
            continue;
        }
        let packet: Value = match serde_json::from_str(raw) {
            Ok(packet) => packet,
            Err(e) => {
                report.rejected += 1;
                report.count("json");
                reject(&mut store.conn, source, raw, &format!("json:{e}"))?;
                continue;
            }
        };

        let errors = validate(&packet);
        if !errors.is_empty() {
            report.rejected += 1;
            for e in &errors {
                report.count(e.split(':').next().unwrap_or(e));
            }
            reject(
                &mut store.conn,
                source,
                raw,
                &format!("contract:{}", errors.join(",")),
            )?;
            continue;
        }

        let site = packet["site_id"].as_str().unwrap_or_default();
        let node = packet["node_id"].as_str().unwrap_or_default();
        let seq = packet["seq"].as_i64().unwrap_or(0);

        let seen: Option<i64> = store
            .conn
            .query_row(
                "SELECT 1 FROM raw_packet WHERE site_id=? AND node_id=? AND seq=?",
                params![site, node, seq],
                |r| r.get(0),
            )
            .optional()?;
        if seen.is_some() {
            report.duplicates += 1;
            report.count("duplicate");
            reject(&mut store.conn, source, raw, "duplicate:seq")?;
            continue;
        }

        let max_seq: Option<i64> = store
            .conn
            .query_row(
                "SELECT max(seq) FROM raw_packet WHERE site_id=? AND node_id=?",
                params![site, node],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if max_seq.is_some_and(|max| seq < max) {
            report.out_of_order += 1; // queued recovery: accept, don't reorder
        }

        let inserted = store.conn.execute(
            "INSERT INTO raw_packet (site_id, node_id, seq, node_role, ts_device,
             received_at, sensor_status, battery_v, signal_dbm, payload)
             VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![
                site,
                node,
                seq,
                packet["node_role"].as_str().unwrap_or_default(),
                packet["ts_device"].as_str().unwrap_or_default(),
                now(),
                packet["sensor_status"].as_str().unwrap_or_default(),
                packet["battery_v"].as_f64(),
                packet["signal_dbm"].as_f64(),
                raw
            ],
        );
        match inserted {
            Ok(_) => report.accepted += 1,
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == ErrorCode::ConstraintViolation =>
            {
                report.duplicates += 1;
                report.count("duplicate");
                reject(&mut store.conn, source, raw, "duplicate:seq")?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(report)
}

#[derive(Debug)]
pub struct Health {
    pub site_id: String,
    pub node_id: String,
    pub node_role: String,
    pub seq: i64,
    pub ts_device: String,
    pub received_at: String,
    pub sensor_status: String,
    pub battery_v: Option<f64>,
    pub signal_dbm: Option<f64>,
}

/// Newest accepted packet per node, for movement-vs-reference comparison.
pub fn latest_health(store: &Store) -> Result<Vec<Health>> {
    let mut stmt = store.conn.prepare(
        "SELECT site_id, node_id, node_role, seq, ts_device, received_at,
         sensor_status, battery_v, signal_dbm
         FROM raw_packet WHERE id IN
         (SELECT max(id) FROM raw_packet GROUP BY site_id, node_id)
         ORDER BY node_role, node_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Health {
            site_id: r.get(0)?,
            node_id: r.get(1)?,
            node_role: r.get(2)?,
            seq: r.get(3)?,
            ts_device: r.get(4)?,
            received_at: r.get(5)?,
            sensor_status: r.get(6)?,
            battery_v: r.get(7)?,
            signal_dbm: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn load_jsonl(path: &str) -> std::io::Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect())
}

/// Reason of the newest reject row. The receiver returns it as the 422 detail;
/// `Report.errors` only carries the coarse prefix.
pub fn last_reject_reason(store: &Store) -> Result<Option<String>> {
    Ok(store
        .conn
        .query_row(
            "SELECT reason FROM rejected ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?)
}
