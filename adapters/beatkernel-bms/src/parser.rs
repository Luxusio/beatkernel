use crate::{
    rational::{Ratio, decimal, gcd},
    *,
};
use beatkernel::{audio::SampleId, chart::*, time::Duration};
use std::collections::BTreeMap;
struct Row {
    measure: usize,
    channel: u8,
    tokens: Vec<u16>,
    line: usize,
}
#[derive(Clone)]
struct Event {
    beat: Ratio,
    cell_end: Option<Ratio>,
    channel: u8,
    value: u16,
    line: usize,
    ordinal: u64,
}
#[derive(Clone)]
struct TimedEvent {
    tick: i64,
    cell_end: Option<i64>,
    channel: u8,
    value: u16,
    line: usize,
    ordinal: u64,
}
struct Note {
    head: TimedEvent,
    end: Option<TimedEvent>,
    lane: BmsLane,
}
fn fail(line: usize, kind: BmsErrorKind) -> BmsError {
    BmsError::new(line, kind)
}
fn arithmetic<T>(line: usize, result: Result<T, BmsErrorKind>) -> Result<T, BmsError> {
    result.map_err(|kind| fail(line, kind))
}
fn bpm(value: Ratio, line: usize) -> Result<Bpm, BmsError> {
    let n = u32::try_from(value.n).map_err(|_| fail(line, BmsErrorKind::Overflow))?;
    let d = u32::try_from(value.d).map_err(|_| fail(line, BmsErrorKind::Overflow))?;
    Bpm::new(n, d).map_err(|_| {
        fail(
            line,
            BmsErrorKind::Syntax("BPM must be positive and fit u32 rational"),
        )
    })
}
fn code(token: &str, radix: u32, line: usize) -> Result<u16, BmsError> {
    if token.len() != 2 || !token.is_ascii() {
        return Err(fail(
            line,
            BmsErrorKind::Syntax("index requires two ASCII digits"),
        ));
    }
    index(token, radix, line)
}
fn index(token: &str, radix: u32, line: usize) -> Result<u16, BmsError> {
    token.bytes().try_fold(0u16, |value, byte| {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'A'..=b'Z' => u32::from(byte - b'A') + 10,
            b'a'..=b'z' => u32::from(byte - b'a') + if radix == 62 { 36 } else { 10 },
            _ => return Err(fail(line, BmsErrorKind::Syntax("invalid index digit"))),
        };
        if digit >= radix {
            return Err(fail(line, BmsErrorKind::Syntax("invalid index digit")));
        }
        // Both callers admit at most two digits, with radix at most 62.
        Ok(value * radix as u16 + digit as u16)
    })
}
fn crop(value: &str, sugar: bool, radix: u32, line: usize) -> Result<BgaCrop, BmsError> {
    let mut fields = value.split_whitespace();
    let source = fields.next().unwrap_or("");
    if !(1..=2).contains(&source.len()) || !source.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(fail(
            line,
            BmsErrorKind::Syntax("BGA source requires one or two resource digits"),
        ));
    }
    let source = ImageId(index(source, radix, line)?);
    let mut coordinates = [0i32; 6];
    for coordinate in &mut coordinates {
        *coordinate = fields
            .next()
            .ok_or_else(|| fail(line, BmsErrorKind::Syntax("BGA requires seven fields")))?
            .parse()
            .map_err(|_| {
                fail(
                    line,
                    BmsErrorKind::Syntax("BGA coordinate must fit signed i32"),
                )
            })?;
    }
    if fields.next().is_some() {
        return Err(fail(
            line,
            BmsErrorKind::Syntax("BGA requires seven fields"),
        ));
    }
    let [x, y, mut right, mut bottom, dx, dy] = coordinates;
    if sugar {
        if right <= 0 || bottom <= 0 {
            return Err(fail(
                line,
                BmsErrorKind::Syntax("BGA width and height must be positive"),
            ));
        }
        right = x
            .checked_add(right)
            .ok_or_else(|| fail(line, BmsErrorKind::Overflow))?;
        bottom = y
            .checked_add(bottom)
            .ok_or_else(|| fail(line, BmsErrorKind::Overflow))?;
    }
    let crop = BgaCrop {
        source,
        source_rect: [x, y, right, bottom],
        destination: [dx, dy],
    };
    crop.validate()
        .map_err(|_| fail(line, BmsErrorKind::Syntax("invalid BGA source rectangle")))?;
    Ok(crop)
}
fn define<K: Ord, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    line: usize,
    label: &'static str,
    policy: DuplicatePolicy,
) -> Result<(), BmsError> {
    if policy == DuplicatePolicy::Reject && map.contains_key(&key) {
        return Err(fail(line, BmsErrorKind::Duplicate(label)));
    }
    map.insert(key, value);
    Ok(())
}
fn visible(channel: u8) -> bool {
    matches!(channel, 0x11..=0x19 | 0x21..=0x29)
}
fn long(channel: u8) -> bool {
    matches!(channel, 0x51..=0x59 | 0x61..=0x69)
}
fn invisible(channel: u8) -> bool {
    matches!(channel, 0x31..=0x39 | 0x41..=0x49)
}
fn mine(channel: u8) -> bool {
    matches!(channel, 0xd1..=0xd9 | 0xe1..=0xe9)
}
fn lane(channel: u8) -> BmsLane {
    BmsLane(if long(channel) {
        channel - 0x40
    } else {
        channel
    })
}
fn sample(value: u16, line: usize, samples: &BTreeMap<u16, String>) -> Result<SampleId, BmsError> {
    if !samples.contains_key(&value) {
        return Err(fail(
            line,
            BmsErrorKind::MissingDefinition {
                kind: "WAV",
                index: value,
            },
        ));
    }
    Ok(SampleId(u64::from(value)))
}
fn update_resolution(
    resolution: &mut i128,
    denominator: i128,
    cap: u32,
    line: usize,
) -> Result<(), BmsError> {
    *resolution = (*resolution / gcd(*resolution, denominator))
        .checked_mul(denominator)
        .ok_or_else(|| fail(line, BmsErrorKind::Resolution))?;
    if *resolution > i128::from(cap) {
        return Err(fail(line, BmsErrorKind::Resolution));
    }
    Ok(())
}

/// Parses the documented deterministic UTF-8 subset without asset IO.
/// Selected BASE headers set a strict file-wide resource radix of 16, 36 or 62;
/// absent headers default to 36, and late headers also govern earlier resources.
/// Unsupported timing/gameplay commands reject with original line diagnostics.
pub fn parse(text: &str, options: ParseOptions) -> Result<BmsChart, BmsError> {
    parse_seeded(text, options, 0)
}

/// Resolve conditional branches with the documented SplitMix64 seed before
/// discovering file-wide BASE and parsing selected payload. Discarded lines
/// still obey physical input caps and never change the selected radix.
pub fn parse_seeded(text: &str, options: ParseOptions, seed: u64) -> Result<BmsChart, BmsError> {
    if options.max_bytes == 0
        || options.max_lines == 0
        || options.max_line_bytes == 0
        || options.max_objects == 0
        || options.max_resolution == 0
        || options.max_objects > MAX_SOURCE_ITEMS
    {
        return Err(fail(0, BmsErrorKind::Limit("invalid parser limits")));
    }
    if text.len() > options.max_bytes {
        return Err(fail(0, BmsErrorKind::Limit("input bytes")));
    }
    // Resolve randomness exactly once. Borrow selected payload within the
    // physical caps so a late BASE can govern the entire selected chart.
    let mut selected = Vec::new();
    let mut conditional = crate::conditional::Conditional::new(seed);
    for (index, original) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = index + 1;
        if line > options.max_lines {
            return Err(fail(line, BmsErrorKind::Limit("line count")));
        }
        if original.len() > options.max_line_bytes {
            return Err(fail(line, BmsErrorKind::Limit("line bytes")));
        }
        let trimmed = original.trim();
        let Some(command_line) = trimmed.strip_prefix('#') else {
            continue;
        };
        if conditional.payload(command_line, line)? {
            selected
                .try_reserve(1)
                .map_err(|_| fail(line, BmsErrorKind::Limit("selected payload allocation")))?;
            selected.push((line, command_line));
        }
    }
    conditional.finish()?;
    let mut resource_radix = 36;
    let mut radix_defined = false;
    for &(line, command_line) in &selected {
        let split = command_line
            .find(char::is_whitespace)
            .unwrap_or(command_line.len());
        if command_line[..split].eq_ignore_ascii_case("BASE") {
            if radix_defined && options.duplicates == DuplicatePolicy::Reject {
                return Err(fail(line, BmsErrorKind::Duplicate("BASE")));
            }
            resource_radix = match command_line[split..].trim() {
                "16" => 16,
                "36" => 36,
                "62" => 62,
                _ => {
                    return Err(fail(
                        line,
                        BmsErrorKind::Syntax("BASE requires 16, 36 or 62"),
                    ));
                }
            };
            radix_defined = true;
        }
    }
    let mut base = Bpm::new(130, 1).expect("valid documented default");
    let mut base_defined = false;
    let mut samples = BTreeMap::new();
    let mut images = BTreeMap::new();
    let mut bga_crops = BTreeMap::new();
    let mut tempos = BTreeMap::new();
    let mut stops = BTreeMap::new();
    let mut lengths = BTreeMap::<usize, (Ratio, usize)>::new();
    let mut metadata = BTreeMap::new();
    let mut lnobj = None;
    let mut warnings = Vec::new();
    let mut rows = Vec::new();
    let mut visual_rows = Vec::new();
    let mut invisible_rows = Vec::new();
    let mut mine_rows = Vec::new();
    let mut raw_count = 0usize;
    let mut max_measure = 0usize;
    for (line, command_line) in selected {
        if command_line.len() >= 6
            && command_line.as_bytes()[..3].iter().all(u8::is_ascii_digit)
            && command_line.as_bytes()[5] == b':'
        {
            let measure = command_line[..3]
                .parse::<usize>()
                .map_err(|_| fail(line, BmsErrorKind::Syntax("invalid measure")))?;
            let channel = u8::from_str_radix(&command_line[3..5], 16).map_err(|_| {
                fail(
                    line,
                    BmsErrorKind::Unsupported("nonstandard channel".into()),
                )
            })?;
            max_measure = max_measure.max(measure);
            let data = command_line[6..].trim();
            if channel == 2 {
                let length = decimal(data, line)?;
                if length.n == 0 {
                    return Err(fail(
                        line,
                        BmsErrorKind::Syntax("measure length must be positive"),
                    ));
                }
                define(
                    &mut lengths,
                    measure,
                    (length, line),
                    line,
                    "measure length",
                    options.duplicates,
                )?;
                continue;
            }
            if !matches!(channel, 1 | 3 | 4 | 6 | 7 | 8 | 9 | 0x0a | 0x0b..=0x0e)
                && !visible(channel)
                && !long(channel)
                && !invisible(channel)
                && !mine(channel)
            {
                return Err(fail(
                    line,
                    BmsErrorKind::Unsupported(format!("channel {channel:02X}")),
                ));
            }
            if data.is_empty() || !data.is_ascii() || data.len() % 2 != 0 {
                return Err(fail(
                    line,
                    BmsErrorKind::Syntax("channel data requires nonempty pairs"),
                ));
            }
            let radix = if channel == 3 || matches!(channel, 0x0b..=0x0e) {
                16
            } else if mine(channel) {
                36
            } else {
                resource_radix
            };
            let mut tokens = Vec::with_capacity(data.len() / 2);
            for token in data.as_bytes().chunks_exact(2) {
                let token = std::str::from_utf8(token).expect("ASCII checked");
                let value = code(token, radix, line)?;
                if value != 0 {
                    raw_count = raw_count
                        .checked_add(1)
                        .filter(|count| *count <= options.max_objects)
                        .ok_or_else(|| fail(line, BmsErrorKind::Limit("nonzero tokens")))?;
                }
                tokens.push(value);
            }
            let row = Row {
                measure,
                channel,
                tokens,
                line,
            };
            if matches!(channel, 4 | 6 | 7 | 0x0a | 0x0b..=0x0e) {
                visual_rows.push(row);
            } else if invisible(channel) {
                invisible_rows.push(row);
            } else if mine(channel) {
                mine_rows.push(row);
            } else {
                rows.push(row);
            }
            continue;
        }
        let split = command_line
            .find(char::is_whitespace)
            .unwrap_or(command_line.len());
        let original_command = &command_line[..split];
        let command = original_command.to_ascii_uppercase();
        let value = command_line[split..].trim();
        if command == "BASE" {
            // Already validated and resolved file-wide before resource parsing.
            continue;
        } else if command == "BPM" {
            if base_defined && options.duplicates == DuplicatePolicy::Reject {
                return Err(fail(line, BmsErrorKind::Duplicate("base BPM")));
            }
            base = bpm(decimal(value, line)?, line)?;
            base_defined = true;
        } else if command.len() == 5 && command.starts_with("WAV") {
            let id = code(&original_command[3..], resource_radix, line)?;
            if value.is_empty() || value.contains('\0') {
                return Err(fail(
                    line,
                    BmsErrorKind::Syntax("nonempty WAV path required"),
                ));
            }
            define(
                &mut samples,
                id,
                value.to_owned(),
                line,
                "WAV definition",
                options.duplicates,
            )?;
        } else if command.len() == 5 && command.starts_with("BPM") {
            let id = code(&original_command[3..], resource_radix, line)?;
            let tempo = bpm(decimal(value, line)?, line)?;
            define(
                &mut tempos,
                id,
                tempo,
                line,
                "BPM definition",
                options.duplicates,
            )?;
        } else if command.len() == 6 && command.starts_with("STOP") {
            let id = code(&original_command[4..], resource_radix, line)?;
            let duration = decimal(value, line)?;
            define(
                &mut stops,
                id,
                duration,
                line,
                "STOP definition",
                options.duplicates,
            )?;
        } else if command == "LNOBJ" {
            let marker = code(value, resource_radix, line)?;
            if marker == 0 {
                return Err(fail(
                    line,
                    BmsErrorKind::Syntax("LNOBJ marker must be nonzero"),
                ));
            }
            define(
                &mut metadata,
                command,
                value.to_owned(),
                line,
                "LNOBJ",
                options.duplicates,
            )?;
            lnobj = Some(marker);
        } else if command == "CANVASSIZE" {
            if let Some([width, height]) = parse_canvas_size(value) {
                metadata.insert(command, format!("{width} {height}"));
            } else {
                warnings.push(BmsWarning { line, message: "invalid CANVASSIZE ignored; requires two positive one-to-four digit ASCII decimals".into() });
            }
        } else if command == "POORBGA" {
            PoorBgaMode::parse(value).map_err(|_| {
                fail(
                    line,
                    BmsErrorKind::Syntax("POORBGA requires exactly 0, 1 or 2"),
                )
            })?;
            define(
                &mut metadata,
                command,
                value.to_owned(),
                line,
                "POORBGA",
                options.duplicates,
            )?;
        } else if command == "LNTYPE" {
            if !matches!(value, "1" | "01" | "2" | "02") {
                return Err(fail(
                    line,
                    BmsErrorKind::Unsupported(format!("LNTYPE {value}")),
                ));
            }
            define(
                &mut metadata,
                command,
                value.to_owned(),
                line,
                "LNTYPE",
                options.duplicates,
            )?;
        } else if matches!(
            command.as_str(),
            "TITLE"
                | "SUBTITLE"
                | "ARTIST"
                | "SUBARTIST"
                | "GENRE"
                | "PLAYER"
                | "RANK"
                | "TOTAL"
                | "PLAYLEVEL"
                | "DIFFICULTY"
                | "COMMENT"
                | "MAKER"
                | "STAGEFILE"
                | "BANNER"
                | "BACKBMP"
        ) {
            define(
                &mut metadata,
                command,
                value.to_owned(),
                line,
                "metadata header",
                options.duplicates,
            )?;
        } else if command == "VOLWAV" {
            parse_wav_gain(value, line)?;
            define(
                &mut metadata,
                command,
                value.to_owned(),
                line,
                "VOLWAV",
                options.duplicates,
            )?;
        } else if command.len() == 5 && command.starts_with("BMP") {
            let id = ImageId(code(&original_command[3..], resource_radix, line)?);
            if value.is_empty() || value.contains('\0') {
                return Err(fail(
                    line,
                    BmsErrorKind::Syntax("nonempty BMP path required"),
                ));
            }
            define(
                &mut images,
                id,
                value.to_owned(),
                line,
                "BMP definition",
                options.duplicates,
            )?;
        } else if (command.len() == 5 && command.starts_with("BGA"))
            || (command.len() == 6 && command.starts_with("@BGA"))
        {
            let sugar = command.starts_with('@');
            let id = ImageId(code(
                &original_command[if sugar { 4 } else { 3 }..],
                resource_radix,
                line,
            )?);
            define(
                &mut bga_crops,
                id,
                crop(value, sugar, resource_radix, line)?,
                line,
                "BGA definition",
                options.duplicates,
            )?;
        } else if command.starts_with("BGA") || command.starts_with("@BGA") {
            warnings.push(BmsWarning {
                line,
                message: format!("visual directive #{command} is not rendered by this adapter"),
            });
        } else {
            return Err(fail(
                line,
                BmsErrorKind::Unsupported(format!("directive #{command}")),
            ));
        }
    }
    let cell_long_notes = matches!(metadata.get("LNTYPE").map(String::as_str), Some("2" | "02"));
    let mut origins = Vec::with_capacity(max_measure + 2);
    let mut durations = Vec::with_capacity(max_measure + 1);
    let mut position = Ratio::ZERO;
    let mut resolution = 1i128;
    for measure in 0..=max_measure {
        let (ratio, line) = lengths.get(&measure).copied().unwrap_or((Ratio::ONE, 0));
        let duration = arithmetic(line, ratio.mul(Ratio { n: 4, d: 1 }))?;
        origins.push(position);
        durations.push(duration);
        update_resolution(&mut resolution, position.d, options.max_resolution, line)?;
        update_resolution(&mut resolution, duration.d, options.max_resolution, line)?;
        position = arithmetic(line, position.add(duration))?;
    }
    origins.push(position);
    update_resolution(&mut resolution, position.d, options.max_resolution, 0)?;
    let mut events = Vec::new();
    let mut ordinal = 0u64;
    for row in rows {
        for (index, value) in row
            .tokens
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value)| *value != 0)
        {
            let offset = arithmetic(
                row.line,
                durations[row.measure].mul(Ratio {
                    n: index as i128,
                    d: row.tokens.len() as i128,
                }),
            )?;
            let beat = arithmetic(row.line, origins[row.measure].add(offset))?;
            update_resolution(&mut resolution, beat.d, options.max_resolution, row.line)?;
            let cell_end = if cell_long_notes && long(row.channel) {
                let end_offset = arithmetic(
                    row.line,
                    durations[row.measure].mul(Ratio {
                        n: (index + 1) as i128,
                        d: row.tokens.len() as i128,
                    }),
                )?;
                let end = arithmetic(row.line, origins[row.measure].add(end_offset))?;
                update_resolution(&mut resolution, end.d, options.max_resolution, row.line)?;
                Some(end)
            } else {
                None
            };
            events.push(Event {
                beat,
                cell_end,
                channel: row.channel,
                value,
                line: row.line,
                ordinal,
            });
            ordinal += 1;
        }
    }
    let mut visual_resolution = resolution;
    let mut visual_events = Vec::new();
    let mut visual_ordinal = 0u64;
    for row in visual_rows {
        for (index, value) in row
            .tokens
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value)| *value != 0)
        {
            let offset = arithmetic(
                row.line,
                durations[row.measure].mul(Ratio {
                    n: index as i128,
                    d: row.tokens.len() as i128,
                }),
            )?;
            let beat = arithmetic(row.line, origins[row.measure].add(offset))?;
            update_resolution(
                &mut visual_resolution,
                beat.d,
                options.max_resolution,
                row.line,
            )?;
            visual_events.push(Event {
                beat,
                cell_end: None,
                channel: row.channel,
                value,
                line: row.line,
                ordinal: visual_ordinal,
            });
            visual_ordinal += 1;
        }
    }
    let visual_resolution =
        u32::try_from(visual_resolution).map_err(|_| fail(0, BmsErrorKind::Resolution))?;
    let mut visual_merged = BTreeMap::new();
    let mut opacity_merged = BTreeMap::new();
    for event in visual_events {
        let tick = arithmetic(event.line, event.beat.ticks(visual_resolution))?;
        if matches!(event.channel, 0x0b..=0x0e) {
            let channel = match event.channel {
                0x0b => BgaChannel::Base,
                0x0c => BgaChannel::Layer,
                0x0d => BgaChannel::Layer2,
                0x0e => BgaChannel::Poor,
                _ => unreachable!("opacity row channel"),
            };
            let marker = BgaOpacityEvent {
                beat: Beat::new(tick)
                    .map_err(|error| fail(event.line, BmsErrorKind::Compile(error)))?,
                channel,
                alpha: u8::try_from(event.value)
                    .map_err(|_| fail(event.line, BmsErrorKind::Overflow))?,
                ordinal: event.ordinal,
            };
            define(
                &mut opacity_merged,
                (tick, channel),
                marker,
                event.line,
                "BGA opacity position",
                options.duplicates,
            )?;
            continue;
        }
        let channel = match event.channel {
            4 => BgaChannel::Base,
            6 => BgaChannel::Poor,
            7 => BgaChannel::Layer,
            0x0a => BgaChannel::Layer2,
            _ => unreachable!("visual row channel"),
        };
        let marker = BgaEvent {
            beat: Beat::new(tick)
                .map_err(|error| fail(event.line, BmsErrorKind::Compile(error)))?,
            channel,
            image: ImageId(event.value),
            ordinal: event.ordinal,
        };
        define(
            &mut visual_merged,
            (tick, channel),
            marker,
            event.line,
            "BGA channel position",
            options.duplicates,
        )?;
    }
    let mut bga: Vec<_> = visual_merged.into_values().collect();
    bga.sort_by_key(|event| (event.beat, event.ordinal));
    let mut bga_opacity: Vec<_> = opacity_merged.into_values().collect();
    bga_opacity.sort_by_key(|event| (event.beat, event.ordinal));
    // Invisible subdivisions never enlarge either gameplay or visual grids.
    let mut invisible_resolution = resolution;
    let mut invisible_events = Vec::new();
    let mut invisible_ordinal = 0u64;
    for row in invisible_rows {
        for (index, value) in row
            .tokens
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value)| *value != 0)
        {
            let offset = arithmetic(
                row.line,
                durations[row.measure].mul(Ratio {
                    n: index as i128,
                    d: row.tokens.len() as i128,
                }),
            )?;
            let beat = arithmetic(row.line, origins[row.measure].add(offset))?;
            update_resolution(
                &mut invisible_resolution,
                beat.d,
                options.max_resolution,
                row.line,
            )?;
            // Validate every selected nonzero reference, even a replaced token.
            let sample = sample(value, row.line, &samples)?;
            invisible_events.push((
                beat,
                BmsLane(row.channel - 0x20),
                sample,
                invisible_ordinal,
                row.line,
            ));
            invisible_ordinal += 1;
        }
    }
    let invisible_resolution =
        u32::try_from(invisible_resolution).map_err(|_| fail(0, BmsErrorKind::Resolution))?;
    let mut invisible_merged = BTreeMap::new();
    for (beat, lane, sample, ordinal, line) in invisible_events {
        let tick = arithmetic(line, beat.ticks(invisible_resolution))?;
        define(
            &mut invisible_merged,
            (tick, lane),
            InvisibleEvent {
                beat: Beat::new(tick).map_err(|error| fail(line, BmsErrorKind::Compile(error)))?,
                lane,
                sample,
                ordinal,
                line,
            },
            line,
            "invisible lane position",
            options.duplicates,
        )?;
    }
    let mut invisible: Vec<_> = invisible_merged.into_values().collect();
    invisible.sort_by_key(|event| (event.beat, event.ordinal));
    // Mine subdivisions have their own grid and never enlarge the gameplay,
    // image or invisible resolutions. Damage tokens are not sample references.
    let mut mine_resolution = resolution;
    let mut mine_events = Vec::new();
    let mut mine_ordinal = 0u64;
    for row in mine_rows {
        for (index, value) in row
            .tokens
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value)| *value != 0)
        {
            let offset = arithmetic(
                row.line,
                durations[row.measure].mul(Ratio {
                    n: index as i128,
                    d: row.tokens.len() as i128,
                }),
            )?;
            let beat = arithmetic(row.line, origins[row.measure].add(offset))?;
            update_resolution(
                &mut mine_resolution,
                beat.d,
                options.max_resolution,
                row.line,
            )?;
            let damage = arithmetic(row.line, MineDamage::from_raw(value))?;
            mine_events
                .try_reserve(1)
                .map_err(|_| fail(row.line, BmsErrorKind::Limit("mine event allocation")))?;
            mine_events.push((
                beat,
                BmsLane(row.channel - 0xc0),
                damage,
                mine_ordinal,
                row.line,
            ));
            mine_ordinal += 1;
        }
    }
    let mine_resolution =
        u32::try_from(mine_resolution).map_err(|_| fail(0, BmsErrorKind::Resolution))?;
    let mut mine_merged = BTreeMap::new();
    for (beat, lane, damage, ordinal, line) in mine_events {
        let tick = arithmetic(line, beat.ticks(mine_resolution))?;
        define(
            &mut mine_merged,
            (tick, lane),
            MineEvent {
                beat: Beat::new(tick).map_err(|error| fail(line, BmsErrorKind::Compile(error)))?,
                lane,
                damage,
                ordinal,
                line,
            },
            line,
            "mine lane position",
            options.duplicates,
        )?;
    }
    let mut mines: Vec<_> = mine_merged.into_values().collect();
    mines.sort_by_key(|event| (event.beat, event.ordinal));
    let resolution = u32::try_from(resolution).map_err(|_| fail(0, BmsErrorKind::Resolution))?;
    let mut measures = Vec::with_capacity(max_measure + 1);
    for measure in 0..=max_measure {
        measures.push(BmsMeasure {
            number: measure as u16,
            start: Beat::new(arithmetic(0, origins[measure].ticks(resolution))?)
                .map_err(|error| fail(0, BmsErrorKind::Compile(error)))?,
            end: Beat::new(arithmetic(0, origins[measure + 1].ticks(resolution))?)
                .map_err(|error| fail(0, BmsErrorKind::Compile(error)))?,
        });
    }
    let mut merged = BTreeMap::<(i64, u8), TimedEvent>::new();
    let mut bgm = Vec::new();
    for event in events {
        let tick = arithmetic(event.line, event.beat.ticks(resolution))?;
        if event.channel == 1 {
            bgm.push(BgmEvent {
                beat: Beat::new(tick)
                    .map_err(|error| fail(event.line, BmsErrorKind::Compile(error)))?,
                sample: sample(event.value, event.line, &samples)?,
                ordinal: event.ordinal,
            });
            continue;
        }
        let timed = TimedEvent {
            tick,
            cell_end: event
                .cell_end
                .map(|end| arithmetic(event.line, end.ticks(resolution)))
                .transpose()?,
            channel: event.channel,
            value: event.value,
            line: event.line,
            ordinal: event.ordinal,
        };
        define(
            &mut merged,
            (tick, event.channel),
            timed,
            event.line,
            "channel position",
            if event.channel == 9 {
                DuplicatePolicy::Reject
            } else {
                options.duplicates
            },
        )?;
    }
    bgm.sort_by_key(|event| (event.beat.ticks(), event.ordinal));
    let mut bpm_events = BTreeMap::<i64, (Bpm, usize)>::new();
    let mut stop_events = BTreeMap::<i64, (Ratio, usize)>::new();
    let mut notes = Vec::new();
    let mut visible_events = BTreeMap::<BmsLane, Vec<TimedEvent>>::new();
    let mut long_events = BTreeMap::<BmsLane, Vec<TimedEvent>>::new();
    for event in merged.into_values() {
        match event.channel {
            3 | 8 => {
                let tempo = if event.channel == 3 {
                    Bpm::new(u32::from(event.value), 1)
                        .map_err(|error| fail(event.line, BmsErrorKind::Compile(error)))?
                } else {
                    *tempos.get(&event.value).ok_or_else(|| {
                        fail(
                            event.line,
                            BmsErrorKind::MissingDefinition {
                                kind: "BPM",
                                index: event.value,
                            },
                        )
                    })?
                };
                if bpm_events.insert(event.tick, (tempo, event.line)).is_some() {
                    return Err(fail(
                        event.line,
                        BmsErrorKind::Duplicate("same-beat tempo channels"),
                    ));
                }
            }
            9 => {
                let duration = *stops.get(&event.value).ok_or_else(|| {
                    fail(
                        event.line,
                        BmsErrorKind::MissingDefinition {
                            kind: "STOP",
                            index: event.value,
                        },
                    )
                })?;
                if stop_events
                    .insert(event.tick, (duration, event.line))
                    .is_some()
                {
                    return Err(fail(event.line, BmsErrorKind::Duplicate("same-beat STOP")));
                }
            }
            channel if long(channel) => long_events.entry(lane(channel)).or_default().push(event),
            _ => visible_events
                .entry(lane(event.channel))
                .or_default()
                .push(event),
        }
    }
    for (lane, mut events) in visible_events {
        events.sort_by_key(|event| (event.tick, event.ordinal));
        let mut pending: Option<TimedEvent> = None;
        for event in events {
            if Some(event.value) == lnobj {
                let head = pending.take().ok_or_else(|| {
                    fail(
                        event.line,
                        BmsErrorKind::LongNote("LNOBJ endpoint has no preceding head"),
                    )
                })?;
                if event.tick <= head.tick {
                    return Err(fail(
                        event.line,
                        BmsErrorKind::LongNote("endpoint must follow head"),
                    ));
                }
                notes.push(Note {
                    head,
                    end: Some(event),
                    lane,
                });
            } else if let Some(head) = pending.replace(event) {
                notes.push(Note {
                    head,
                    end: None,
                    lane,
                });
            }
        }
        if let Some(head) = pending {
            notes.push(Note {
                head,
                end: None,
                lane,
            });
        }
    }
    for (lane, mut markers) in long_events {
        markers.sort_by_key(|event| (event.tick, event.ordinal));
        if cell_long_notes {
            // Exact-position duplicate policy has already selected complete
            // cells above. Union only the retained extents, preserving the
            // first chronological head and never sounding continuation values.
            let mut held: Option<(TimedEvent, TimedEvent)> = None;
            for head in markers {
                let end = TimedEvent {
                    tick: head.cell_end.expect("LNTYPE2 long cell has an exact end"),
                    cell_end: None,
                    value: 0,
                    ..head.clone()
                };
                if let Some((_, previous_end)) = &mut held {
                    if head.tick <= previous_end.tick {
                        if end.tick > previous_end.tick {
                            *previous_end = end;
                        }
                        continue;
                    }
                }
                if let Some((head, end)) = held.replace((head, end)) {
                    notes.push(Note {
                        head,
                        end: Some(end),
                        lane,
                    });
                }
            }
            if let Some((head, end)) = held {
                notes.push(Note {
                    head,
                    end: Some(end),
                    lane,
                });
            }
            continue;
        }
        if markers.len() % 2 != 0 {
            return Err(fail(
                markers.last().expect("odd count").line,
                BmsErrorKind::LongNote("unmatched endpoint"),
            ));
        }
        for pair in markers.chunks_exact(2) {
            if pair[1].tick <= pair[0].tick {
                return Err(fail(
                    pair[1].line,
                    BmsErrorKind::LongNote("endpoint must follow head"),
                ));
            }
            notes.push(Note {
                head: pair[0].clone(),
                end: Some(pair[1].clone()),
                lane,
            });
        }
    }
    let mut ranges = BTreeMap::<BmsLane, Vec<(i64, i64, usize)>>::new();
    for note in &notes {
        if let Some(end) = &note.end {
            ranges
                .entry(note.lane)
                .or_default()
                .push((note.head.tick, end.tick, end.line));
        }
    }
    for ranges in ranges.values_mut() {
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            if pair[1].0 <= pair[0].1 {
                return Err(fail(
                    pair[1].2,
                    BmsErrorKind::LongNote("held lane ranges overlap or touch"),
                ));
            }
        }
    }
    for note in &notes {
        if note.end.is_none()
            && ranges.get(&note.lane).is_some_and(|ranges| {
                ranges
                    .get(
                        ranges
                            .partition_point(|&(start, _, _)| start <= note.head.tick)
                            .wrapping_sub(1),
                    )
                    .is_some_and(|&(_, end, _)| note.head.tick <= end)
            })
        {
            return Err(fail(
                note.head.line,
                BmsErrorKind::LongNote("visible note overlaps held lane"),
            ));
        }
    }
    notes.sort_by_key(|note| (note.head.tick, note.lane, note.head.ordinal));
    notes
        .len()
        .checked_add(bpm_events.len())
        .and_then(|count| count.checked_add(stop_events.len()))
        .and_then(|count| count.checked_add(bgm.len()))
        .and_then(|count| count.checked_add(bga.len()))
        .and_then(|count| count.checked_add(bga_opacity.len()))
        .and_then(|count| count.checked_add(invisible.len()))
        .and_then(|count| count.checked_add(mines.len()))
        .filter(|count| *count <= options.max_objects && *count <= MAX_SOURCE_ITEMS)
        .ok_or_else(|| fail(0, BmsErrorKind::Limit("source items")))?;
    let mut source = SourceChart::new(resolution, base)
        .map_err(|error| fail(0, BmsErrorKind::Compile(error)))?;
    for (&tick, &(tempo, line)) in &bpm_events {
        source.bpm_changes.push(BpmChange {
            beat: Beat::new(tick).map_err(|error| fail(line, BmsErrorKind::Compile(error)))?,
            bpm: tempo,
        });
    }
    for (tick, (duration, line)) in stop_events {
        let active = bpm_events
            .range(..=tick)
            .next_back()
            .map_or(base, |(_, (tempo, _))| *tempo);
        let numerator = duration
            .n
            .checked_mul(60_000_000_000)
            .and_then(|v| v.checked_mul(i128::from(active.denominator())))
            .ok_or_else(|| fail(line, BmsErrorKind::Overflow))?;
        let denominator = duration
            .d
            .checked_mul(48)
            .and_then(|v| v.checked_mul(i128::from(active.numerator())))
            .ok_or_else(|| fail(line, BmsErrorKind::Overflow))?;
        let nanos = i64::try_from(numerator / denominator)
            .map_err(|_| fail(line, BmsErrorKind::Overflow))?;
        source.stops.push(Stop {
            beat: Beat::new(tick).map_err(|error| fail(line, BmsErrorKind::Compile(error)))?,
            duration: Duration::from_nanos(nanos),
        });
    }
    let mut mapped = Vec::with_capacity(notes.len());
    for (index, note) in notes.into_iter().enumerate() {
        let sample = sample(note.head.value, note.head.line, &samples)?;
        let id = ObjectId(index as u64 + 1);
        let end = note
            .end
            .as_ref()
            .map(|event| Beat::new(event.tick))
            .transpose()
            .map_err(|error| fail(note.head.line, BmsErrorKind::Compile(error)))?;
        let tail_sample = note
            .end
            .as_ref()
            .filter(|event| event.value != 0)
            .map(|event| SampleId(u64::from(event.value)));
        let mut bytes = vec![1, note.lane.channel()];
        bytes.extend(note.head.value.to_le_bytes());
        bytes.extend((note.head.line as u64).to_le_bytes());
        if let Some(tail) = tail_sample {
            bytes.push(1);
            bytes.extend((tail.0 as u16).to_le_bytes());
        } else {
            bytes.push(0);
        }
        source.objects.push(SourceObject {
            id,
            start: Beat::new(note.head.tick)
                .map_err(|error| fail(note.head.line, BmsErrorKind::Compile(error)))?,
            end,
            interaction: InteractionId(
                u32::from(note.lane.channel()) + if end.is_some() { 0x40 } else { 0 },
            ),
            visual: VisualId(u32::from(note.lane.channel())),
            audio: Some(AudioBinding(u32::from(note.head.value))),
            metadata: ObjectMetadata(bytes),
        });
        mapped.push(BmsNote {
            object: id,
            lane: note.lane,
            sample,
            tail_sample,
            line: note.head.line,
        });
    }
    Ok(BmsChart {
        source,
        samples,
        images,
        bga_crops,
        bga,
        bga_opacity,
        bga_ticks_per_beat: visual_resolution,
        invisible,
        invisible_ticks_per_beat: invisible_resolution,
        mines,
        mine_ticks_per_beat: mine_resolution,
        notes: mapped,
        bgm,
        metadata,
        warnings,
        measures,
    })
}
