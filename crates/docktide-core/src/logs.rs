//! Log streaming: line assembly, batching, and resume (spec M1 FR-4).

use std::collections::VecDeque;
use std::pin::pin;
use std::time::Duration;

use bollard::container::LogOutput;
use bollard::errors::Error as BollardError;
use bollard::query_parameters::LogsOptions;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use tokio::time::{Instant, sleep_until};

use crate::connection::DockerService;
use crate::error::{DockerError, IpcError};
use crate::events::stream_failure;
use crate::streams::SubscriptionId;

/// A batch leaves at most this long after its first line, and batches leave no
/// more often than this.
pub(crate) const BATCH_INTERVAL: Duration = Duration::from_millis(50);
/// Lines per batch. With `BATCH_INTERVAL` this caps delivery at 10,000 lines/s.
pub(crate) const BATCH_LINES: usize = 500;
/// Lines waiting for delivery; beyond this the oldest are dropped.
pub(crate) const QUEUE_LINES: usize = 10_000;
/// A partial line this long is emitted as a line, to bound memory.
pub(crate) const MAX_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LogStream {
    Stdout,
    Stderr,
    /// A TTY container's single stream.
    Console,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogLine {
    pub stream: LogStream,
    /// RFC 3339 with nanoseconds, UTC, as Docker sent it.
    pub timestamp: String,
    /// May contain SGR sequences (colours and text attributes), nothing else.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LogEvent {
    Lines {
        lines: Vec<LogLine>,
    },
    /// Lines dropped because they arrived faster than they could be shown.
    Gap {
        #[cfg_attr(test, ts(type = "number"))]
        dropped: u64,
    },
    /// Docker closed the stream: the container stopped or was removed.
    Ended,
    Error {
        error: IpcError,
    },
}

impl DockerService {
    /// Follows a container's logs until the container stops, Docker fails,
    /// `cancel_stream` is called, or `sink` returns false.
    ///
    /// `tail` is a line count, `None` for all. `after` is the timestamp of the
    /// last line already shown; when set, `tail` is ignored and only later lines
    /// are sent. Must be called within a Tokio runtime.
    pub async fn open_logs(
        &self,
        id: &str,
        tail: Option<u32>,
        after: Option<String>,
        sink: impl Fn(LogEvent) -> bool + Send + Sync + 'static,
    ) -> Result<SubscriptionId, DockerError> {
        let (endpoint, docker) = self.docker().await?;
        let options = logs_options(tail, after.as_deref());
        let after = after.as_deref().map(normalize_timestamp);
        let logs = docker.logs(id, Some(options));
        let (subscription, cancel) = self.streams().insert();
        let service = self.clone();
        tokio::spawn(async move {
            tokio::select! {
                end = forward(logs, after, &sink) => {
                    let event = match end {
                        End::Eof => Some(LogEvent::Ended),
                        End::Failed(err) => Some(LogEvent::Error {
                            error: stream_failure(&service, &err, endpoint).into(),
                        }),
                        End::SinkClosed => None,
                    };
                    if let Some(event) = event {
                        sink(event);
                    }
                }
                () = cancel.cancelled() => {}
            }
            service.streams().remove(subscription);
        });
        Ok(subscription)
    }
}

fn logs_options(tail: Option<u32>, after: Option<&str>) -> LogsOptions {
    // Docker's `since` has one-second resolution; `forward` drops the rest.
    let since = after.and_then(unix_seconds);
    LogsOptions {
        follow: true,
        stdout: true,
        stderr: true,
        timestamps: true,
        since: since.and_then(|s| i32::try_from(s).ok()).unwrap_or(0),
        until: 0,
        tail: match (since, tail) {
            (None, Some(lines)) => lines.to_string(),
            _ => "all".to_owned(),
        },
    }
}

/// Rewrites Docker's RFC 3339 UTC timestamp with a 9-digit fraction, so
/// timestamps compare as strings. Docker trims trailing zeros from the fraction.
fn normalize_timestamp(timestamp: &str) -> String {
    let Some(base) = timestamp.strip_suffix('Z') else {
        return timestamp.to_owned();
    };
    let (seconds, fraction) = base.split_once('.').unwrap_or((base, ""));
    format!("{seconds}.{fraction:0<9}Z")
}

/// Whole Unix seconds of an RFC 3339 UTC timestamp, for Docker's `since`.
fn unix_seconds(timestamp: &str) -> Option<i64> {
    let field = |range: std::ops::Range<usize>| timestamp.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (field(0..4)?, field(5..7)?, field(8..10)?);
    let (hour, minute, second) = (field(11..13)?, field(14..16)?, field(17..19)?);
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Decodes one line for display: lossy UTF-8; text after the last inner `\r`;
/// SGR sequences kept; other escapes and control characters except tab removed.
fn clean(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    let text = decoded.trim_end_matches('\r');
    let text = text.rsplit('\r').next().unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    let mut sequence = String::from("\x1b[");
                    for c in chars.by_ref() {
                        sequence.push(c);
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                    let params = &sequence[2..sequence.len() - 1];
                    let is_sgr = sequence.ends_with('m')
                        && params
                            .chars()
                            .all(|c| c.is_ascii_digit() || c == ';' || c == ':');
                    if is_sgr {
                        out.push_str(&sequence);
                    }
                }
                // OSC, DCS, SOS, PM, APC: skip to BEL or ST (`ESC \`).
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                // Charset designation: `ESC ( B` and friends.
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                }
                _ => {}
            },
            '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Turns Docker's log output into lines (FR-4 line assembly).
#[derive(Debug, Default)]
struct Assembler {
    /// A message Docker split over frames, for stdout and stderr.
    partial: [Option<Partial>; 2],
    /// Raw TTY bytes up to the next newline.
    console: Vec<u8>,
    /// The timestamp of a TTY line cut at the cap; its rest has none.
    console_cut: Option<String>,
}

#[derive(Debug)]
struct Partial {
    timestamp: String,
    bytes: Vec<u8>,
}

impl Assembler {
    fn push(&mut self, output: LogOutput, lines: &mut Vec<LogLine>) {
        match output {
            LogOutput::StdOut { message } => self.frame(LogStream::Stdout, &message, lines),
            LogOutput::StdErr { message } => self.frame(LogStream::Stderr, &message, lines),
            LogOutput::Console { message } => self.console(&message, lines),
            LogOutput::StdIn { .. } => {}
        }
    }

    /// Emits what is left when the stream ends.
    fn finish(&mut self, lines: &mut Vec<LogLine>) {
        for (slot, stream) in [(0, LogStream::Stdout), (1, LogStream::Stderr)] {
            if let Some(partial) = self.partial[slot].take()
                && !partial.bytes.is_empty()
            {
                lines.push(LogLine {
                    stream,
                    timestamp: partial.timestamp,
                    text: clean(&partial.bytes),
                });
            }
        }
        if !self.console.is_empty() {
            let rest = std::mem::take(&mut self.console);
            self.console_line(&rest, lines);
        }
    }

    /// One multiplexed frame: `<timestamp> <payload>`. A payload without a
    /// trailing newline is continued by the next frame of the same stream.
    fn frame(&mut self, stream: LogStream, frame: &[u8], lines: &mut Vec<LogLine>) {
        let slot = &mut self.partial[usize::from(stream == LogStream::Stderr)];
        let (timestamp, payload) = split_timestamp(frame);
        let partial = slot.get_or_insert_with(|| Partial {
            timestamp: timestamp.to_owned(),
            bytes: Vec::new(),
        });
        partial.bytes.extend_from_slice(payload);
        while let Some(end) = partial.bytes.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = partial.bytes.drain(..=end).collect();
            lines.push(LogLine {
                stream,
                timestamp: partial.timestamp.clone(),
                text: clean(&line[..end]),
            });
        }
        if partial.bytes.len() >= MAX_LINE_BYTES {
            lines.push(LogLine {
                stream,
                timestamp: partial.timestamp.clone(),
                text: clean(&std::mem::take(&mut partial.bytes)),
            });
        }
        if payload.ends_with(b"\n") {
            *slot = None;
        }
    }

    /// Raw TTY bytes in arbitrary chunks; every line starts with a timestamp.
    fn console(&mut self, chunk: &[u8], lines: &mut Vec<LogLine>) {
        self.console.extend_from_slice(chunk);
        while let Some(end) = self.console.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.console.drain(..=end).collect();
            self.console_line(&line[..end], lines);
        }
        if self.console.len() >= MAX_LINE_BYTES {
            let line = std::mem::take(&mut self.console);
            let timestamp = self.console_line(&line, lines);
            self.console_cut = Some(timestamp);
        }
    }

    /// Emits one TTY line and returns its timestamp.
    fn console_line(&mut self, line: &[u8], lines: &mut Vec<LogLine>) -> String {
        let (timestamp, text) = if let Some(timestamp) = self.console_cut.take() {
            (timestamp, line)
        } else {
            let (timestamp, text) = split_timestamp(line);
            (timestamp.to_owned(), text)
        };
        lines.push(LogLine {
            stream: LogStream::Console,
            timestamp: timestamp.clone(),
            text: clean(text),
        });
        timestamp
    }
}

/// Splits `<timestamp> <rest>`. Text without a leading timestamp keeps it all.
fn split_timestamp(bytes: &[u8]) -> (&str, &[u8]) {
    let Some(space) = bytes.iter().take(40).position(|&b| b == b' ') else {
        return ("", bytes);
    };
    match std::str::from_utf8(&bytes[..space]) {
        Ok(timestamp) if timestamp.starts_with(|c: char| c.is_ascii_digit()) => {
            (timestamp, &bytes[space + 1..])
        }
        _ => ("", bytes),
    }
}

/// Lines waiting for delivery. Full means the oldest line is dropped and counted.
#[derive(Debug)]
struct Queue {
    lines: VecDeque<LogLine>,
    dropped: u64,
    capacity: usize,
}

impl Queue {
    fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            dropped: 0,
            capacity,
        }
    }

    fn push(&mut self, line: LogLine) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(line);
    }

    /// The dropped count since the last batch, and up to `max` lines.
    fn take(&mut self, max: usize) -> (u64, Vec<LogLine>) {
        let count = self.lines.len().min(max);
        (
            std::mem::take(&mut self.dropped),
            self.lines.drain(..count).collect(),
        )
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.dropped == 0
    }
}

/// Why `forward` returned.
#[derive(Debug)]
enum End {
    /// Docker closed the stream.
    Eof,
    Failed(BollardError),
    /// The subscriber is gone.
    SinkClosed,
}

/// Sends `logs` as batches of lines until it ends. Lines at or before `after`
/// (normalized) are dropped. Lines still queued are sent before returning on
/// `Eof` or `Failed`.
async fn forward(
    logs: impl Stream<Item = Result<LogOutput, BollardError>>,
    mut after: Option<String>,
    sink: &(dyn Fn(LogEvent) -> bool + Send + Sync),
) -> End {
    let mut logs = pin!(logs);
    let mut assembler = Assembler::default();
    let mut queue = Queue::new(QUEUE_LINES);
    let mut new_lines = Vec::new();
    // When the oldest queued line arrived, and when the last batch left.
    let mut oldest: Option<Instant> = None;
    let mut last_sent: Option<Instant> = None;
    // Sends one batch; false when the sink is gone.
    let send = |queue: &mut Queue| {
        let (dropped, lines) = queue.take(BATCH_LINES);
        (dropped == 0 || sink(LogEvent::Gap { dropped }))
            && (lines.is_empty() || sink(LogEvent::Lines { lines }))
    };
    let drain = |queue: &mut Queue| {
        while !queue.is_empty() {
            if !send(queue) {
                return false;
            }
        }
        true
    };
    loop {
        let due = oldest.map(|oldest| {
            let paced = last_sent.map(|at| at + BATCH_INTERVAL);
            if queue.lines.len() >= BATCH_LINES {
                paced.unwrap_or(oldest)
            } else {
                paced.map_or(oldest + BATCH_INTERVAL, |p| p.max(oldest + BATCH_INTERVAL))
            }
        });
        let next_batch = async {
            match due {
                Some(at) => sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = next_batch => {
                if !send(&mut queue) {
                    return End::SinkClosed;
                }
                let now = Instant::now();
                last_sent = Some(now);
                oldest = (!queue.is_empty()).then_some(now);
            }
            item = logs.next() => {
                let end = match item {
                    Some(Ok(output)) => {
                        assembler.push(output, &mut new_lines);
                        None
                    }
                    Some(Err(err)) => Some(End::Failed(err)),
                    None => Some(End::Eof),
                };
                if end.is_some() {
                    assembler.finish(&mut new_lines);
                }
                for line in new_lines.drain(..) {
                    // Docker's `since` is whole seconds; drop what was already shown.
                    if let Some(shown) = &after {
                        if normalize_timestamp(&line.timestamp) <= *shown {
                            continue;
                        }
                        after = None;
                    }
                    queue.push(line);
                    oldest.get_or_insert_with(Instant::now);
                }
                if let Some(end) = end {
                    return if drain(&mut queue) { end } else { End::SinkClosed };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::stream;
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;

    const TS: &str = "2026-10-22T04:38:47.825201379Z";

    fn stdout(text: &str) -> LogOutput {
        LogOutput::StdOut {
            message: Bytes::from(text.to_owned()),
        }
    }

    fn stderr(text: &str) -> LogOutput {
        LogOutput::StdErr {
            message: Bytes::from(text.to_owned()),
        }
    }

    fn console(bytes: &[u8]) -> LogOutput {
        LogOutput::Console {
            message: Bytes::copy_from_slice(bytes),
        }
    }

    fn line(stream: LogStream, timestamp: &str, text: &str) -> LogLine {
        LogLine {
            stream,
            timestamp: timestamp.into(),
            text: text.into(),
        }
    }

    fn assemble(outputs: Vec<LogOutput>) -> Vec<LogLine> {
        let mut assembler = Assembler::default();
        let mut lines = Vec::new();
        for output in outputs {
            assembler.push(output, &mut lines);
        }
        assembler.finish(&mut lines);
        lines
    }

    // --- text ---

    #[test]
    fn clean_keeps_plain_text_and_tabs() {
        assert_eq!(clean(b"hello\tworld"), "hello\tworld");
    }

    #[test]
    fn clean_handles_carriage_returns_like_a_terminal() {
        assert_eq!(clean(b"done\r"), "done");
        assert_eq!(clean(b"done\r\r"), "done");
        assert_eq!(clean(b"10%\r50%\r100%"), "100%");
        assert_eq!(clean(b"a\rb\r\r"), "b");
    }

    #[test]
    fn clean_keeps_sgr_and_strips_other_escapes() {
        assert_eq!(clean(b"\x1b[31mred\x1b[0m"), "\x1b[31mred\x1b[0m");
        assert_eq!(clean(b"\x1b[1;38;5;208mx\x1b[m"), "\x1b[1;38;5;208mx\x1b[m");
        assert_eq!(clean(b"\x1b[38:2::255:0:0mx"), "\x1b[38:2::255:0:0mx");
        // Erase line, cursor movement, private modes.
        assert_eq!(clean(b"\x1b[2Kok\x1b[1A\x1b[?25l"), "ok");
        // OSC title, terminated by BEL or ST; OSC 8 hyperlink.
        assert_eq!(clean(b"\x1b]0;title\x07text"), "text");
        assert_eq!(
            clean(b"\x1b]8;;https://x.test\x1b\\link\x1b]8;;\x1b\\"),
            "link"
        );
        // Charset designation and other two-byte escapes.
        assert_eq!(clean(b"\x1b(Bplain\x1b7"), "plain");
        // Unterminated sequences at the end of a line.
        assert_eq!(clean(b"cut\x1b[31"), "cut");
        assert_eq!(clean(b"cut\x1b"), "cut");
    }

    #[test]
    fn clean_strips_control_characters_and_decodes_lossily() {
        assert_eq!(clean(b"a\x00b\x07c\x7fd"), "abcd");
        assert_eq!(clean("a\u{9b}31mb".as_bytes()), "a31mb");
        assert_eq!(clean(b"bad \xff byte"), "bad \u{fffd} byte");
    }

    // --- timestamps ---

    #[test]
    fn timestamps_normalize_to_nine_fraction_digits() {
        assert_eq!(normalize_timestamp(TS), TS);
        assert_eq!(
            normalize_timestamp("2026-10-22T04:38:47.8252Z"),
            "2026-10-22T04:38:47.825200000Z"
        );
        assert_eq!(
            normalize_timestamp("2026-10-22T04:38:47Z"),
            "2026-10-22T04:38:47.000000000Z"
        );
        assert!(normalize_timestamp("2026-10-22T04:38:47.8Z") < normalize_timestamp(TS));
    }

    #[test]
    fn unix_seconds_of_a_timestamp() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_seconds("2026-10-22T04:38:47.9Z"), Some(1_792_643_927));
        assert_eq!(unix_seconds("2000-02-29T12:00:00.5Z"), Some(951_825_600));
        assert_eq!(unix_seconds("yesterday"), None);
    }

    #[test]
    fn options_follow_with_timestamps_and_resume_from_the_second() {
        let fresh = logs_options(Some(1000), None);
        assert!(fresh.follow && fresh.stdout && fresh.stderr && fresh.timestamps);
        assert_eq!((fresh.tail.as_str(), fresh.since), ("1000", 0));
        assert_eq!(logs_options(None, None).tail, "all");

        let resumed = logs_options(Some(1000), Some("2026-10-22T04:38:47.9Z"));
        assert_eq!(
            (resumed.tail.as_str(), resumed.since),
            ("all", 1_792_643_927)
        );
    }

    // --- assembly ---

    #[test]
    fn frames_become_lines_per_stream() {
        let lines = assemble(vec![
            stdout(&format!("{TS} hello\n")),
            stderr(&format!("{TS} oops\n")),
        ]);
        assert_eq!(
            lines,
            [
                line(LogStream::Stdout, TS, "hello"),
                line(LogStream::Stderr, TS, "oops"),
            ]
        );
    }

    #[test]
    fn a_split_message_continues_without_the_second_timestamp() {
        let later = "2026-10-22T04:38:48Z";
        let lines = assemble(vec![
            stdout(&format!("{TS} first half ")),
            stderr(&format!("{later} interleaved\n")),
            stdout(&format!("{later} second half\n")),
        ]);
        assert_eq!(
            lines,
            [
                line(LogStream::Stderr, later, "interleaved"),
                line(LogStream::Stdout, TS, "first half second half"),
            ]
        );
    }

    #[test]
    fn crlf_lines_lose_the_carriage_return() {
        let lines = assemble(vec![stdout(&format!("{TS} windows\r\n"))]);
        assert_eq!(lines, [line(LogStream::Stdout, TS, "windows")]);
    }

    #[test]
    fn tty_chunks_split_anywhere_including_inside_a_timestamp() {
        let raw = format!("{TS} partial-rest\r\r\n{TS} a\rb\r\r\n{TS} \x1b[31mred\x1b[0m\r\r\n");
        let bytes = raw.as_bytes();
        let lines = assemble(vec![
            console(&bytes[..10]),
            console(&bytes[10..50]),
            console(&bytes[50..]),
        ]);
        assert_eq!(
            lines,
            [
                line(LogStream::Console, TS, "partial-rest"),
                line(LogStream::Console, TS, "b"),
                line(LogStream::Console, TS, "\x1b[31mred\x1b[0m"),
            ]
        );
    }

    #[test]
    fn a_partial_line_is_emitted_when_the_stream_ends() {
        let lines = assemble(vec![stdout(&format!("{TS} no newline")), console(b"")]);
        assert_eq!(lines, [line(LogStream::Stdout, TS, "no newline")]);
        let tty = assemble(vec![console(format!("{TS} prompt> ").as_bytes())]);
        assert_eq!(tty, [line(LogStream::Console, TS, "prompt> ")]);
    }

    #[test]
    fn overlong_lines_are_cut_at_the_cap() {
        let long = "x".repeat(MAX_LINE_BYTES + 10);
        let lines = assemble(vec![
            stdout(&format!("{TS} {long}")),
            stdout(&format!("{TS} tail\n")),
        ]);
        assert_eq!(
            lines.len(),
            2,
            "{:?}",
            lines.iter().map(|l| l.text.len()).collect::<Vec<_>>()
        );
        assert!(lines[0].text.len() >= MAX_LINE_BYTES);
        assert_eq!(lines[1], line(LogStream::Stdout, TS, "tail"));

        let tty = assemble(vec![
            console(format!("{TS} {long}").as_bytes()),
            console(b"tail\r\n"),
        ]);
        assert_eq!(tty.len(), 2);
        assert_eq!(tty[1], line(LogStream::Console, TS, "tail"));
    }

    // --- queue ---

    #[test]
    fn the_queue_drops_the_oldest_and_counts_them() {
        let mut queue = Queue::new(3);
        for n in 0..5 {
            queue.push(line(LogStream::Stdout, TS, &n.to_string()));
        }
        let (dropped, lines) = queue.take(2);
        assert_eq!(dropped, 2);
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["2", "3"]
        );
        assert_eq!(queue.take(10), (0, vec![line(LogStream::Stdout, TS, "4")]));
    }

    // --- serialization ---

    #[test]
    fn events_serialize_to_the_ipc_shape() {
        let lines = LogEvent::Lines {
            lines: vec![line(LogStream::Stderr, TS, "x")],
        };
        assert_eq!(
            serde_json::to_value(lines).unwrap(),
            json!({ "type": "lines", "lines": [{ "stream": "stderr", "timestamp": TS, "text": "x" }] })
        );
        assert_eq!(
            serde_json::to_value(LogEvent::Gap { dropped: 7 }).unwrap(),
            json!({ "type": "gap", "dropped": 7 })
        );
        assert_eq!(
            serde_json::to_value(LogEvent::Ended).unwrap(),
            json!({ "type": "ended" })
        );
    }

    // --- delivery (paused time) ---

    type Sent = std::sync::Arc<std::sync::Mutex<Vec<(Duration, LogEvent)>>>;

    /// A sink recording each event with the paused time since `start`.
    fn recorder(start: Instant) -> (Sent, impl Fn(LogEvent) -> bool + Send + Sync) {
        let sent = Sent::default();
        let log = sent.clone();
        (sent, move |event| {
            log.lock().unwrap().push((start.elapsed(), event));
            true
        })
    }

    fn feed() -> (
        mpsc::UnboundedSender<Result<LogOutput, BollardError>>,
        impl Stream<Item = Result<LogOutput, BollardError>>,
    ) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        (tx, stream::poll_fn(move |cx| rx.poll_recv(cx)))
    }

    fn numbered(n: usize) -> LogOutput {
        stdout(&format!("{TS} {n}\n"))
    }

    fn batch_sizes(sent: &Sent) -> Vec<(u128, usize)> {
        sent.lock()
            .unwrap()
            .iter()
            .filter_map(|(at, event)| match event {
                LogEvent::Lines { lines } => Some((at.as_millis(), lines.len())),
                _ => None,
            })
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn lines_are_batched_for_50_ms_after_the_first() {
        let start = Instant::now();
        let (sent, sink) = recorder(start);
        let (tx, logs) = feed();
        let task = async {
            tx.send(Ok(numbered(1))).unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            tx.send(Ok(numbered(2))).unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            tx.send(Ok(numbered(3))).unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(tx);
        };
        let (end, ()) = tokio::join!(forward(logs, None, &sink), task);
        assert!(matches!(end, End::Eof), "{end:?}");
        assert_eq!(batch_sizes(&sent), [(50, 2), (180, 1)]);
    }

    #[tokio::test(start_paused = true)]
    async fn bursts_leave_in_500_line_batches_at_most_every_50_ms() {
        let start = Instant::now();
        let (sent, sink) = recorder(start);
        let logs = stream::iter((0..1200).map(|n| Ok(numbered(n)))).chain(stream::pending());
        let _ = tokio::time::timeout(Duration::from_secs(1), forward(logs, None, &sink)).await;
        assert_eq!(batch_sizes(&sent), [(0, 500), (50, 500), (100, 200)]);
    }

    #[tokio::test(start_paused = true)]
    async fn overflow_drops_the_oldest_and_reports_a_gap_first() {
        let start = Instant::now();
        let (sent, sink) = recorder(start);
        let total = BATCH_LINES + QUEUE_LINES + 25;
        let logs = stream::iter((0..total).map(|n| Ok(numbered(n)))).chain(stream::pending());
        let _ = tokio::time::timeout(Duration::from_secs(2), forward(logs, None, &sink)).await;
        let sent = sent.lock().unwrap();
        assert!(
            matches!(sent[1].1, LogEvent::Gap { dropped: 25 }),
            "{:?}",
            sent[1]
        );
        let LogEvent::Lines { lines } = &sent[2].1 else {
            panic!("expected lines after the gap");
        };
        // The first 500 left at once; the next 25 were the oldest queued.
        assert_eq!(lines[0].text, (BATCH_LINES + 25).to_string());
        let shown: usize = sent
            .iter()
            .map(|(_, e)| match e {
                LogEvent::Lines { lines } => lines.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(shown, total - 25);
    }

    #[tokio::test(start_paused = true)]
    async fn the_end_flushes_queued_lines_at_once() {
        let start = Instant::now();
        let (sent, sink) = recorder(start);
        let logs = stream::iter((0..1200).map(|n| Ok(numbered(n))));
        let end = forward(logs, None, &sink).await;
        assert!(matches!(end, End::Eof));
        assert_eq!(batch_sizes(&sent), [(0, 500), (0, 500), (0, 200)]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failure_flushes_lines_first() {
        let start = Instant::now();
        let (sent, sink) = recorder(start);
        let logs = stream::iter([Ok(numbered(1)), Err(BollardError::RequestTimeoutError)]);
        let end = forward(logs, None, &sink).await;
        assert!(matches!(
            end,
            End::Failed(BollardError::RequestTimeoutError)
        ));
        assert_eq!(batch_sizes(&sent), [(0, 1)]);
    }

    #[tokio::test(start_paused = true)]
    async fn resume_drops_lines_up_to_and_including_after() {
        let start = Instant::now();
        let (sent, sink) = recorder(start);
        let logs = stream::iter([
            Ok(stdout("2026-10-22T04:38:47.5Z old\n")),
            Ok(stdout("2026-10-22T04:38:47.825201379Z shown last time\n")),
            Ok(stdout("2026-10-22T04:38:47.9Z new\n")),
            Ok(stdout("2026-10-22T04:38:48Z newer\n")),
        ]);
        forward(logs, Some(normalize_timestamp(TS)), &sink).await;
        let sent = sent.lock().unwrap();
        let LogEvent::Lines { lines } = &sent[0].1 else {
            panic!("expected lines");
        };
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["new", "newer"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_sink_stops_forwarding() {
        let logs = stream::iter((0..10).map(|n| Ok(numbered(n)))).chain(stream::pending());
        let end = forward(logs, None, &|_| false).await;
        assert!(matches!(end, End::SinkClosed));
    }
}
