//! BMKG area forecasts. No telemetry, model, or decision-engine inputs.
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeDelta, Utc};
use rig_reqwest::reqwest;
use serde_json::{Value, json};
use tokio::runtime::Builder;

const ENDPOINT: &str = "https://api.bmkg.go.id/publik/prakiraan-cuaca";
const MAX_BYTES: usize = 128 * 1024;
const REFRESH: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);

pub(super) struct Weather {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    adm4: Option<String>,
    forecast: Option<Forecast>,
    failed: bool,
    generation: u64,
}

struct Forecast {
    fetched_at: DateTime<Utc>,
    area: String,
    city: Option<String>,
    timezone: Option<String>,
    slots: Vec<(DateTime<Utc>, Value)>,
}

impl Weather {
    fn new(adm4: Option<String>) -> Self {
        Self {
            state: Mutex::new(State {
                adm4,
                ..State::default()
            }),
            wake: Condvar::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn disabled() -> Self {
        Self::new(None)
    }

    pub(super) fn start(adm4: Option<String>) -> Arc<Self> {
        let weather = Arc::new(Self::new(adm4));
        let worker = Arc::clone(&weather);
        // ponytail: one site, one in-memory cache. Persist forecasts only if a
        // future study needs to align archived model runs with forecast cycles.
        if thread::Builder::new()
            .name("bmkg".into())
            .spawn(move || worker.run())
            .is_err()
        {
            eprintln!("BMKG unavailable: could not start forecast worker");
            weather
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .failed = true;
        }
        weather
    }

    /// Switches the forecast area and wakes the worker for an immediate fetch.
    pub(super) fn set_adm4(&self, adm4: String) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.adm4 = Some(adm4);
        state.forecast = None;
        state.failed = false;
        state.generation = state.generation.wrapping_add(1);
        self.wake.notify_all();
    }

    fn run(&self) {
        let runtime = Builder::new_current_thread().enable_all().build();
        let http = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(8))
            .build();
        let (Ok(runtime), Ok(http)) = (runtime, http) else {
            eprintln!("BMKG unavailable: could not initialize HTTPS client");
            self.state.lock().unwrap_or_else(|e| e.into_inner()).failed = true;
            return;
        };
        loop {
            let (adm4, generation) = self.area();
            let forecast = runtime
                .block_on(fetch(&http, ENDPOINT, &adm4))
                .and_then(|body| Forecast::parse(&body, &adm4, Utc::now()));
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.generation != generation {
                continue;
            }
            state.failed = forecast.is_none();
            if forecast.is_some() {
                state.forecast = forecast;
            }
            let delay = if state.failed { RETRY } else { REFRESH };
            let _ = self.wake.wait_timeout(state, delay);
        }
    }

    fn area(&self) -> (String, u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(adm4) = &state.adm4 {
                return (adm4.clone(), state.generation);
            }
            state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(super) fn snapshot(&self, now: DateTime<Utc>) -> Value {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(adm4) = &state.adm4 else {
            return json!({"status": "disabled"});
        };
        let Some(forecast) = &state.forecast else {
            let status = if state.failed {
                "unavailable"
            } else {
                "loading"
            };
            return json!({"status": status, "source": "BMKG", "adm4": adm4, "slots": []});
        };
        let slots: Vec<_> = forecast
            .slots
            .iter()
            .filter(|(valid_at, _)| *valid_at >= now)
            .take(3)
            .map(|(_, slot)| slot)
            .collect();
        let status = if slots.is_empty() {
            "unavailable"
        } else if state.failed || now - forecast.fetched_at > TimeDelta::hours(1) {
            "stale"
        } else {
            "ready"
        };
        json!({"status": status, "source": "BMKG", "adm4": adm4,
            "area": forecast.area, "city": forecast.city, "timezone": forecast.timezone,
            "fetched_at": iso(forecast.fetched_at), "slots": slots})
    }
}

pub(super) fn valid_adm4(code: &str) -> bool {
    let mut parts = code.split('.');
    [2, 2, 2, 4].into_iter().all(|len| {
        parts
            .next()
            .is_some_and(|part| part.len() == len && part.bytes().all(|b| b.is_ascii_digit()))
    }) && parts.next().is_none()
}

fn iso(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn text(value: &Value) -> Option<&str> {
    let text = value.as_str()?.trim();
    (!text.is_empty() && text.chars().count() <= 120 && !text.chars().any(char::is_control))
        .then_some(text)
}

// BMKG returns either an IANA name or a bare UTC offset such as "+0700".
// Indonesian offsets map onto fixed IANA zones; anything else is unknown.
fn timezone(value: &Value) -> Option<String> {
    match text(value)? {
        "+0700" => Some("Asia/Jakarta"),
        "+0800" => Some("Asia/Makassar"),
        "+0900" => Some("Asia/Jayapura"),
        value if value.contains('/') => Some(value),
        _ => None,
    }
    .map(str::to_owned)
}

impl Forecast {
    fn parse(body: &Value, adm4: &str, now: DateTime<Utc>) -> Option<Self> {
        let location = &body["lokasi"];
        if location["adm4"].as_str()? != adm4 {
            return None;
        }
        let group = body["data"]
            .as_array()?
            .iter()
            .find(|group| group["lokasi"]["adm4"].as_str() == Some(adm4))?;
        let mut slots: Vec<_> = group["cuaca"]
            .as_array()?
            .iter()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|row| {
                let valid_at = NaiveDateTime::parse_from_str(
                    row["utc_datetime"].as_str()?,
                    "%Y-%m-%d %H:%M:%S",
                )
                .ok()?
                .and_utc();
                let issued_at = NaiveDateTime::parse_from_str(
                    row["analysis_date"].as_str()?.trim_end_matches('Z'),
                    "%Y-%m-%dT%H:%M:%S",
                )
                .ok()?
                .and_utc();
                if valid_at < now || issued_at > now || now - issued_at > TimeDelta::hours(24) {
                    return None;
                }
                let condition = text(&row["weather_desc"])?;
                let temperature = row["t"].as_f64().filter(|t| (-90.0..=60.0).contains(t));
                let humidity = row["hu"].as_f64().filter(|h| (0.0..=100.0).contains(h));
                // ponytail: BMKG does not document `tp`; expose the raw value
                // unlabelled. Confirm its unit before using it as a model input.
                let precipitation = row["tp"].as_f64().filter(|p| (0.0..=500.0).contains(p));
                Some((
                    valid_at,
                    json!({"valid_at": iso(valid_at), "issued_at": iso(issued_at),
                    "condition_id": condition, "condition_en": text(&row["weather_desc_en"]),
                    "temperature_c": temperature, "humidity_pct": humidity,
                    "precipitation": precipitation}),
                ))
            })
            .collect();
        slots.sort_by_key(|(valid_at, _)| *valid_at);
        slots.dedup_by_key(|(valid_at, _)| *valid_at);
        (!slots.is_empty()).then_some(Self {
            fetched_at: now,
            area: text(&location["desa"])?.to_owned(),
            city: text(&location["kotkab"]).map(str::to_owned),
            timezone: timezone(&location["timezone"]),
            slots,
        })
    }
}

async fn fetch(http: &reqwest::Client, endpoint: &str, adm4: &str) -> Option<Value> {
    let mut url = reqwest::Url::parse(endpoint).ok()?;
    url.query_pairs_mut().append_pair("adm4", adm4);
    let mut response = http
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > MAX_BYTES - body.len() {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    const ADM4: &str = "31.71.03.1001";

    fn date(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn publish(weather: &Weather, forecast: Option<Forecast>) {
        let mut state = weather.state.lock().unwrap();
        state.failed = forecast.is_none();
        if let Some(forecast) = forecast {
            state.forecast = Some(forecast);
        }
    }

    fn fixture() -> Value {
        json!({
            "lokasi": {"adm4": ADM4, "desa": "Kemayoran", "kotkab": "Jakarta Pusat", "timezone": "Asia/Jakarta"},
            "data": [{"lokasi": {"adm4": ADM4}, "cuaca": [[
                {"utc_datetime": "2026-11-28 06:00:00", "analysis_date": "2026-11-28T00:00:00", "weather_desc": "Hujan Ringan", "weather_desc_en": "Light Rain", "t": 29, "hu": 74, "tp": 2},
                {"utc_datetime": "2026-11-28 00:00:00", "analysis_date": "2026-11-28T00:00:00", "weather_desc": "Berawan"}
            ], [
                {"utc_datetime": "2026-11-28 03:00:00", "analysis_date": "2026-11-28T00:00:00", "weather_desc": "Berawan", "weather_desc_en": "Cloudy", "hu": null}
            ]]}]
        })
    }

    #[test]
    fn bmkg_offset_timezone_is_normalized_for_local_display() {
        let now = date("2026-11-28T01:00:00Z");
        for (tz, expected) in [
            ("+0700", Some("Asia/Jakarta")),
            ("+0800", Some("Asia/Makassar")),
            ("+0900", Some("Asia/Jayapura")),
            ("Asia/Jakarta", Some("Asia/Jakarta")),
            ("+0530", None),
            ("+07:00", None),
        ] {
            let mut body = fixture();
            body["lokasi"]["timezone"] = json!(tz);
            let forecast = Forecast::parse(&body, ADM4, now).unwrap();
            assert_eq!(forecast.timezone.as_deref(), expected, "for {tz:?}");
        }
    }

    #[test]
    fn bmkg_response_shows_ordered_upcoming_forecasts_through_an_outage() {
        let now = date("2026-11-28T01:00:00Z");
        let weather = Weather::new(Some(ADM4.to_owned()));
        assert_eq!(weather.snapshot(now)["status"], "loading");
        publish(&weather, Forecast::parse(&fixture(), ADM4, now));
        let view = weather.snapshot(now);
        assert_eq!(view["status"], "ready");
        assert_eq!(view["source"], "BMKG");
        assert_eq!(view["adm4"], ADM4);
        assert_eq!(view["timezone"], "Asia/Jakarta");
        assert_eq!(view["fetched_at"], "2026-11-28T01:00:00Z");
        let slots = view["slots"].as_array().unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0]["valid_at"], "2026-11-28T03:00:00Z");
        assert_eq!(slots[0]["issued_at"], "2026-11-28T00:00:00Z");
        assert!(slots[0]["humidity_pct"].is_null());
        assert_eq!(slots[1]["condition_en"], "Light Rain");
        assert_eq!(slots[1]["temperature_c"], 29.0);
        assert_eq!(slots[1]["humidity_pct"], 74.0);
        assert_eq!(slots[1]["precipitation"], 2.0);
        assert!(slots[1].get("tp").is_none());
        publish(&weather, None);
        let stale = weather.snapshot(now);
        assert_eq!(stale["status"], "stale");
        assert_eq!(stale["slots"], view["slots"]);
        let expired = weather.snapshot(date("2026-11-28T07:00:00Z"));
        assert_eq!(expired["status"], "unavailable");
        assert_eq!(expired["slots"], json!([]));
    }

    #[test]
    fn rejects_wrong_area_and_unusable_forecasts_without_inventing_weather() {
        let now = date("2026-11-28T01:00:00Z");
        for invalid in [
            "31.71.3.1001",
            "31.71.03.1001&x=1",
            "31.71.03.1001 ",
            "３１.71.03.1001",
            "",
        ] {
            assert!(!valid_adm4(invalid), "accepted {invalid:?}");
        }
        assert!(valid_adm4(ADM4));
        assert!(Forecast::parse(&fixture(), "31.71.01.1001", now).is_none());
        let mut wrong_group = fixture();
        wrong_group["data"][0]["lokasi"]["adm4"] = json!("31.71.01.1001");
        assert!(Forecast::parse(&wrong_group, ADM4, now).is_none());
        assert!(
            Forecast::parse(&json!({"lokasi": {"adm4": ADM4}, "data": []}), ADM4, now).is_none()
        );
        assert!(Forecast::parse(&fixture(), ADM4, date("2026-11-30T01:00:00Z")).is_none());
        let weather = Weather::new(Some(ADM4.to_owned()));
        publish(&weather, None);
        assert_eq!(weather.snapshot(now)["status"], "unavailable");
    }

    #[test]
    fn changing_area_clears_the_previous_forecast_and_reloads() {
        let now = date("2026-11-28T01:00:00Z");
        let weather = Weather::new(Some(ADM4.to_owned()));
        assert_eq!(weather.snapshot(now)["status"], "loading");
        publish(&weather, Forecast::parse(&fixture(), ADM4, now));
        assert_eq!(weather.snapshot(now)["status"], "ready");

        let mut other = fixture();
        other["lokasi"]["adm4"] = json!("31.71.01.1001");
        other["lokasi"]["desa"] = json!("Gambir");
        other["lokasi"]["timezone"] = json!("+0700");
        other["data"][0]["lokasi"]["adm4"] = json!("31.71.01.1001");
        weather.set_adm4("31.71.01.1001".into());
        let loading = weather.snapshot(now);
        assert_eq!(loading["status"], "loading");
        assert_eq!(loading["adm4"], "31.71.01.1001");
        assert_eq!(loading["slots"], json!([]));
        publish(&weather, Forecast::parse(&other, "31.71.01.1001", now));
        let ready = weather.snapshot(now);
        assert_eq!(ready["status"], "ready");
        assert_eq!(ready["area"], "Gambir");
        assert_eq!(ready["adm4"], "31.71.01.1001");
    }

    #[test]
    fn unconfigured_weather_is_disabled_not_loading() {
        let weather = Weather::new(None);
        assert_eq!(weather.snapshot(Utc::now()), json!({"status": "disabled"}));
    }

    #[test]
    fn fetch_sends_only_adm4_and_accepts_only_successful_bounded_json() {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        for (status, body, ok) in [
            (200, fixture().to_string(), true),
            (429, fixture().to_string(), false),
            (200, "{".into(), false),
            (200, " ".repeat(128 * 1024 + 1), false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!(
                "http://{}/publik/prakiraan-cuaca",
                listener.local_addr().unwrap()
            );
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert_eq!(
                    line.trim_end(),
                    format!("GET /publik/prakiraan-cuaca?adm4={ADM4} HTTP/1.1")
                );
                loop {
                    line.clear();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    assert!(!line.to_ascii_lowercase().starts_with("authorization:"));
                }
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                let _ = stream.write_all(body.as_bytes());
            });
            assert_eq!(runtime.block_on(fetch(&http, &url, ADM4)).is_some(), ok);
            server.join().unwrap();
        }
    }
}
