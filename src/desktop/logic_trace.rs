//! A data-only reader for digital Value Change Dump files. Keep assignments in
//! file order: two records at the same tick need not describe the same state.
use super::native_preview::Cancellation;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, str::SplitAsciiWhitespace};

#[path = "logic_trace/view.rs"]
mod view;
pub(super) use view::View;
pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 131_072;

struct Declaration {
    path: String,
    kind: String,
    signal: usize,
}
struct Signal {
    code: String,
    width: usize,
    primary: usize,
    records: Vec<usize>,
}
struct Record {
    tick: u64,
    signal: usize,
    value: Box<str>,
}
pub(super) struct Trace {
    declarations: Vec<Declaration>,
    signals: Vec<Signal>,
    records: Vec<Record>,
    scale: Option<(u8, &'static str)>,
    last_tick: u64,
    digest: String,
    source: String,
}

type Words<'a> = SplitAsciiWhitespace<'a>;
fn word<'a>(w: &mut Words<'a>) -> Result<&'a str, String> {
    w.next().ok_or_else(|| "Truncated VCD statement.".into())
}
fn end(w: &mut Words<'_>) -> Result<(), String> {
    if word(w)? != "$end" {
        return Err("Expected a VCD $end delimiter.".into());
    }
    Ok(())
}
fn name<'a>(w: &mut Words<'a>) -> Result<&'a str, String> {
    let s = word(w)?;
    if s.len() > 256 || !s.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("Unsupported or oversized VCD identifier.".into());
    }
    Ok(s)
}
fn scale(w: &mut Words<'_>) -> Result<(u8, &'static str), String> {
    let first = word(w)?;
    let (n, unit) = first.split_at(first.bytes().take_while(u8::is_ascii_digit).count());
    let n = match n {
        "1" => 1,
        "10" => 10,
        "100" => 100,
        _ => return Err("Invalid VCD timescale multiplier.".into()),
    };
    let unit = match if unit.is_empty() { word(w)? } else { unit } {
        "s" => "s",
        "ms" => "ms",
        "us" => "µs",
        "ns" => "ns",
        "ps" => "ps",
        "fs" => "fs",
        _ => return Err("Unknown VCD timescale unit.".into()),
    };
    end(w)?;
    Ok((n, unit))
}
fn suffix(w: &mut Words<'_>) -> Result<String, String> {
    let mut range = String::new();
    loop {
        let s = word(w)?;
        if s == "$end" {
            break;
        }
        if range.len() + s.len() > 64 {
            return Err("Oversized VCD reference range.".into());
        }
        range.push_str(s);
    }
    if !range.is_empty() {
        let inside = range
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or("");
        if inside.split(':').count() > 2 || !inside.split(':').all(|s| s.parse::<i64>().is_ok()) {
            return Err("Unsupported VCD reference range.".into());
        }
    }
    Ok(range)
}
impl Trace {
    pub(super) fn read(bytes: &[u8], cancel: &Cancellation) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("VCD timing preview limit: 8 MiB.".into());
        }
        let text = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes))
            .map_err(|_| "Cannot decode the VCD text.")?;
        let mut w = text.split_ascii_whitespace();
        let mut declarations = Vec::new();
        let mut signals: Vec<Signal> = Vec::new();
        let mut records = Vec::new();
        let mut ids: HashMap<&str, usize> = HashMap::new();
        let mut scopes = Vec::new();
        let mut timescale = None;
        let mut tick = 0;
        let mut definitions = true;
        let mut dump = false;
        while let Some(s) = w.next() {
            if cancel.is_cancelled() {
                return Err("Preview cancelled.".into());
            }
            if s == "$comment" || (definitions && matches!(s, "$date" | "$version")) {
                if !w.any(|s| s == "$end") {
                    return Err("Unterminated VCD declaration or comment.".into());
                }
            } else if definitions {
                match s {
                    "$scope" => {
                        if scopes.len() == 32 {
                            return Err("VCD preview limit: 32 nested scopes.".into());
                        }
                        let _kind = name(&mut w)?;
                        scopes.push(name(&mut w)?);
                        end(&mut w)?;
                    }
                    "$upscope" => {
                        scopes.pop().ok_or("Unexpected VCD $upscope.")?;
                        end(&mut w)?;
                    }
                    "$timescale" if timescale.is_none() => timescale = Some(scale(&mut w)?),
                    "$var" => {
                        if declarations.len() == 4096 {
                            return Err("VCD preview limit: 4,096 declarations.".into());
                        }
                        let kind = word(&mut w)?;
                        if !matches!(
                            kind,
                            "wire"
                                | "reg"
                                | "logic"
                                | "integer"
                                | "time"
                                | "parameter"
                                | "bit"
                                | "tri"
                                | "tri0"
                                | "tri1"
                                | "triand"
                                | "trior"
                                | "trireg"
                                | "wand"
                                | "wor"
                                | "supply0"
                                | "supply1"
                        ) {
                            return Err("This VCD contains an unsupported variable type; only digital state is displayed.".into());
                        }
                        let width = word(&mut w)?
                            .parse::<usize>()
                            .map_err(|_| "Invalid VCD variable width.")?;
                        if !(1..=256).contains(&width) {
                            return Err(
                                "VCD preview supports digital widths from 1 to 256 bits.".into()
                            );
                        }
                        let code = name(&mut w)?;
                        let local = name(&mut w)?;
                        let mut path = scopes.join(".");
                        if !path.is_empty() {
                            path.push('.');
                        }
                        path.push_str(local);
                        path.push_str(&suffix(&mut w)?);
                        if path.len() > 1024 {
                            return Err("VCD signal path exceeds the preview limit.".into());
                        }
                        let signal = *ids.entry(code).or_insert_with(|| {
                            signals.push(Signal {
                                code: code.into(),
                                width,
                                primary: declarations.len(),
                                records: Vec::new(),
                            });
                            signals.len() - 1
                        });
                        if signals[signal].width != width {
                            return Err("VCD aliases have inconsistent widths.".into());
                        }
                        declarations.push(Declaration {
                            path,
                            kind: kind.into(),
                            signal,
                        });
                    }
                    "$enddefinitions" if scopes.is_empty() => {
                        end(&mut w)?;
                        definitions = false;
                    }
                    _ => return Err("Unsupported or out-of-order VCD declaration.".into()),
                }
            } else if let Some(time) = s.strip_prefix('#') {
                let next = time.parse::<u64>().map_err(|_| "Invalid VCD timestamp.")?;
                if dump || !time.bytes().all(|b| b.is_ascii_digit()) || next < tick {
                    return Err("Invalid or decreasing VCD timestamp.".into());
                }
                tick = next;
            } else if matches!(s, "$dumpvars" | "$dumpall") && !dump {
                dump = true;
            } else if s == "$end" && dump {
                dump = false;
            } else {
                // Do not bridge a recording gap or silently discard unsupported values.
                let (code, value, scalar) = match s.as_bytes()[0] {
                    b'b' | b'B' => (word(&mut w)?, &s[1..], false),
                    b'0' | b'1' | b'x' | b'X' | b'z' | b'Z' => (&s[1..], &s[..1], true),
                    _ => {
                        return Err(
                            "Unsupported VCD value or dump directive; no partial timing shown."
                                .into(),
                        )
                    }
                };
                let signal = *ids
                    .get(code)
                    .ok_or("VCD assignment uses an undeclared identifier.")?;
                let sig = &mut signals[signal];
                if value.is_empty()
                    || value.len() > sig.width
                    || (scalar && sig.width != 1)
                    || !value
                        .bytes()
                        .all(|b| matches!(b.to_ascii_lowercase(), b'0' | b'1' | b'x' | b'z'))
                {
                    return Err("Invalid or over-width VCD logic value.".into());
                }
                if records.len() == MAX_RECORDS {
                    return Err("VCD preview limit: 131,072 assignments.".into());
                }
                sig.records.push(records.len());
                records.push(Record {
                    tick,
                    signal,
                    value: value.to_ascii_lowercase().into_boxed_str(),
                });
            }
        }
        if definitions || dump || declarations.is_empty() {
            return Err("Unfinished or missing VCD declarations or dump block.".into());
        }
        Ok(Self {
            declarations,
            signals,
            records,
            scale: timescale,
            last_tick: tick,
            digest: format!("{:x}", Sha256::digest(bytes)),
            source: text.to_owned(),
        })
    }

    fn time(&self, ticks: i128) -> String {
        let (n, unit) = self.scale.unwrap_or((1, "ticks"));
        format!("{} {unit}", ticks * i128::from(n))
    }
    fn value(&self, row: usize) -> String {
        let r = &self.records[row];
        let lead = match r.value.as_bytes()[0] {
            b'x' => "x",
            b'z' => "z",
            _ => "0",
        };
        lead.repeat(self.signals[r.signal].width - r.value.len()) + &r.value
    }
    fn state(&self, signal: usize, before_row: usize) -> String {
        let rows = &self.signals[signal].records;
        let end = rows.partition_point(|r| *r < before_row);
        if end == 0 {
            "not recorded".into()
        } else {
            self.value(rows[end - 1])
        }
    }
    fn assignment(&self, row: usize) -> String {
        let r = &self.records[row];
        let sig = &self.signals[r.signal];
        format!(
            "#{} · {} · {} [{}]: {} → {}",
            row + 1,
            self.time(r.tick.into()),
            self.declarations[sig.primary].path,
            sig.code,
            self.state(r.signal, row),
            self.value(row)
        )
    }
    fn interval(&self, start: usize, stop: usize) -> String {
        self.time(i128::from(self.records[stop].tick) - i128::from(self.records[start].tick))
    }
    fn report(&self, file: &str, row: usize, reference: Option<usize>) -> String {
        let mut out = format!(
            "{file}\nSHA-256: {}\nVCD · {}\nCursor: {}\n",
            self.digest,
            self.scale.map_or_else(
                || "timescale not declared".to_owned(),
                |_| format!("1 tick = {}", self.time(1))
            ),
            self.assignment(row)
        );
        if let Some(a) = reference {
            out.push_str(&format!(
                "Reference: {}\nCursor − reference: {}\n",
                self.assignment(a),
                self.interval(a, row)
            ));
        }
        out
    }
}
