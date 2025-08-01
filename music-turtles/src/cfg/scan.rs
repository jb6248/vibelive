/*

Informally, line comments starting with `//` are allowed.

Grammar := `start ` NonTerminal `\n` Production*

Production := NonTerminal `=` MusicString

MusicString := MusicPrimitive*

MusicPrimitive :=
  | Symbol
  | `{` (MusicString `|`)* MusicString? `}`
  | `[` MusicTransform `][` MusicString `]`

MusicTransform :=
    | `x` usize
    | `T` Int
    | `>>` Fraction

Symbol :=
  | NonTerminal
  | `:` Terminal

NonTerminal := [-a-zA-Z1-9/#\?]

Terminal :=
  | Note (`<` Duration `>`)?
  | `:` MetaControl

Note :=
  | `_`
  | Int?[a-gA-G](b|#)?

MetaControl :=
  | `i=` Instrument
  | `v=` Volume

Instrument := Sine | piano | ...

Volume := Int

------ Examples --------

```
start S
S = [x3][:4c<1> :4d :_ :f# :g :c ::i=piano B]
B = :0c
```

*/
use std::collections::HashSet;
use num::rational::Ratio;
use crate::cfg::{Grammar, MetaControl, MusicPrimitive, MusicString, MusicTransform, NonTerminal, Production, Symbol, Terminal, TerminalNote};
use crate::composition::{Instrument, Octave, Pitch, Volume};
use crate::time::{Beat, MusicTime, TimeCompression};


#[derive(Debug)]
pub enum ScanError {
    Generic(String),
    ExpectedEither(String, String),
}

pub type Result<T> = std::result::Result<T, ScanError>;

pub trait Scanner {
    type Output;
    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)>;
}

type ScanPrefix = String;

pub struct GrammarScanner;

pub struct ProductionScanner;

pub struct MusicStringScanner;

pub struct MusicPrimitiveScanner;
pub struct MusicPrimitiveSplitScanner;
pub struct MusicPrimitiveRepeatScanner;
pub struct MusicTransformScanner;

pub struct SymbolScanner;

pub struct NonTerminalScanner;

pub struct TerminalScanner;

pub struct NoteScanner;

pub struct DurationScanner;
pub struct FractionScanner;

pub struct MetaControlScanner;

pub struct InstrumentScanner;

pub struct VolumeScanner;

impl Scanner for GrammarScanner {
    type Output = Grammar;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        let lines = input.lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .filter(|line| !line.trim().starts_with("//"))
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return Err(ScanError::Generic("Expected at least one line".to_string()));
        }
        let start_line = lines[0];
        let start = start_line
            .strip_prefix("start ")
            .ok_or_else(|| ScanError::Generic("Expected 'start' at the beginning of the first line".to_string()))?;
        let start = NonTerminalScanner.scan(start)
            .map(|(nt, _s)| NonTerminal::Custom(nt))?;
        let productions = lines[1..]
            .iter()
            .map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return Ok(None);
                }
                let (prod, _s) = ProductionScanner.scan(line)?;
                Ok(Some(prod))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|x| x)
            .collect();
        Ok((Grammar { start, productions }, ""))
    }
}

impl Scanner for ProductionScanner {
    type Output = Production;
    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        scan_map(concat(
            scan_map(
                concat(NonTerminalScanner, trim(StringScanner("=".to_string()))),
                |(nt, _s)| NonTerminal::Custom(nt),
            ),
            MusicStringScanner,
        ), |(nt, str)| Production(nt, str))
            .scan(input)
    }
}

impl Scanner for MusicStringScanner {
    type Output = MusicString;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        let mut music_string = Vec::new();
        let mut remaining_input = input;

        while !remaining_input.is_empty() {
            // skip to the first non-whitespace character
            remaining_input = remaining_input.trim_start();
            if remaining_input.is_empty() {
                break;
            }
            match MusicPrimitiveScanner.scan(remaining_input) {
                Ok((primitive, new_input)) => {
                    music_string.push(primitive);
                    remaining_input = new_input;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok((MusicString(music_string), remaining_input))
    }
}

impl Scanner for MusicPrimitiveScanner {
    type Output = MusicPrimitive;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // split scanner, or else repeat scanner, or else SymbolScanner
        disjoint(
            ScanPrefix::from("{".to_string()),
            MusicPrimitiveSplitScanner,
            None,
            disjoint(
                ScanPrefix::from("[".to_string()),
                MusicPrimitiveRepeatScanner,
                None,
                scan_map(SymbolScanner, |s| MusicPrimitive::Simple(s)),
            ),
        )
            .scan(input)
    }
}

impl Scanner for MusicPrimitiveSplitScanner {
    type Output = MusicPrimitive;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // if it starts with '{', then find the matching '}' and split on each '|'
        if let Some('{') = input.chars().next() {
            let rest = &input[1..];
            if let Some(end) = find_matching(rest, '{', '}') {
                let inner = &rest[..end];
                let mut parts = inner.split('|');
                let first_part = parts.next().unwrap_or("");
                let rest_parts: Vec<_> = parts.collect();
                let scanner = consume(MusicStringScanner);
                let (music_string, _consumed) = scanner.scan(first_part)?;
                let rest_music_strings: Vec<_> = rest_parts
                    .iter()
                    .map(|&s| MusicStringScanner.scan(s))
                    .try_fold(vec![music_string], |mut vec, res| {
                        let (music_string, _consumed) = res?;
                        vec.push(music_string);
                        Ok(vec)
                    })?;
                let rest = &rest[end + 1..];
                Ok((MusicPrimitive::Split { branches: rest_music_strings }, rest))
            } else {
                Err(ScanError::Generic("Expected '}'".to_string()))
            }
        } else {
            Err(ScanError::Generic("Expected '{'".to_string()))
        }
    }
}

impl Scanner for MusicPrimitiveRepeatScanner {
    type Output = MusicPrimitive;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // first scan '[' a positive integer, '][', then a MusicString, and finally ']'
        if let Some('[') = input.chars().next() {
            if let Some(repeat_num_end) = input.find(']') {
                let repeat_num = &input[1..repeat_num_end];
                if let Some('[') = &input[repeat_num_end + 1..].chars().next() {
                    let rest = &input[repeat_num_end + 2..];
                    if let Some(end_bracket) = find_matching(rest, '[', ']')
                    {
                        let music_string = &rest[..end_bracket];
                        let scanner = consume(MusicStringScanner);
                        let music_string = scanner.scan(music_string).map(|(ms, _empty)| ms)?;
                        let transform = consume(MusicTransformScanner).scan(repeat_num).map(|(ms, _empty)| ms)?;
                        let rest = &rest[end_bracket + 1..];
                        Ok((
                            MusicPrimitive::Transform {
                                transform,
                                content: music_string,
                            },
                            rest,
                        ))
                    } else {
                        Err(ScanError::Generic("Expected ']'".to_string()))
                    }
                } else {
                    Err(ScanError::Generic("Expected '['".to_string()))
                }
            } else {
                Err(ScanError::Generic("Expected ']'".to_string()))
            }
        } else {
            Err(ScanError::Generic("Expected '['".to_string()))
        }
    }
}

impl Scanner for MusicTransformScanner {
    type Output = MusicTransform;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // if it starts with 'x', then scan a positive integer
        // if it starts with 'T', then scan an integer
        // if it starts with '>>', then scan a Duration
        // otherwise, return an error
        if let Some(first) = input.chars().next() {
            match first {
                'x' => {
                    let num: usize = (&input[1..]).parse().map_err(|_| ScanError::Generic("Expected positive integer after 'x'".to_string()))?;
                    Ok((MusicTransform::Repeat {
                        num,
                    }, ""))
                }
                'T' => {
                    let num = &input[1..];
                    let num = num.parse().map_err(|_| ScanError::Generic("Expected integer after 'T'".to_string()))?;
                    Ok((MusicTransform::Transpose {
                        semitones: num,
                    }, ""))
                }
                '>' if input.starts_with(">>") => {
                    let (fraction, rest) = consume(FractionScanner).scan(&input[2..])
                        .map_err(|_| ScanError::Generic("Expected fraction after '>>'".to_string()))?;
                    Ok((MusicTransform::Compression {
                        // use reciprocal because the user expects the inverse.
                        // ex. If they do `>>2` they expect the music to go twice as fast,
                        //  meaning half the time.
                        factor: TimeCompression(fraction.recip())
                    }, rest))
                }
                _ => Err(ScanError::Generic(format!("Expected MusicTransform but found {first}"))),
            }
        } else {
            Err(ScanError::Generic("Expected MusicTransform".to_string()))
        }
    }
}

impl Scanner for SymbolScanner {
    type Output = Symbol;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // if it starts with ':', use TerminalScanner
        // otherwise, use NonTerminalScanner
        disjoint(
            ScanPrefix::from(":".to_string()),
            scan_map(scan_map_input(TerminalScanner, |s| &s[1..]), |s| {
                Symbol::T(s)
            }),
            None,
            scan_map(NonTerminalScanner, |s| Symbol::NT(NonTerminal::Custom(s))),
        )
            .scan(input)
    }
}

impl Scanner for TerminalScanner {
    type Output = Terminal;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // if it starts with ':', then use MetaControlScanner
        // otherwise, use TerminalNoteScanner
        disjoint(
            ScanPrefix::from(":".to_string()),
            scan_map_input(scan_map(MetaControlScanner, |s| Terminal::Meta(s)), |s| &s[1..]),
            None,
            scan_map(concat(NoteScanner, DurationScanner), |(note, duration)| {
                Terminal::Music {
                    note,
                    duration,
                }
            }),
        )
            .scan(input)
    }
}

impl Scanner for NoteScanner {
    type Output = TerminalNote;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        /*
        Note :=
          | `_`
          | Int?[a-gA-G](b|#)?
        */
        let mut chars = input.chars();
        let mut rest = input;
        let mut octave = 4;
        let mut note = 0;
        let mut consumed = 0;
        if let Some(first) = chars.next() {
            consumed += 1;
            let next = if first == '_' {
                return Ok((TerminalNote::Rest, chars.as_str()));
            } else if let Some(dig) = first.to_digit(10) {
                octave = dig as Octave;
                consumed += 1;
                chars.next()
            } else {
                Some(first)
            };
            if let Some(next) = next {
                if 'a' <= next.to_ascii_lowercase() && next.to_ascii_lowercase() <= 'g' {
                    match next.to_ascii_lowercase() {
                        'a' => note = 0,
                        'b' => note = 2,
                        'c' => note = 3,
                        'd' => note = 5,
                        'e' => note = 7,
                        'f' => note = 8,
                        'g' => note = 10,
                        _ => unreachable!(),
                    }
                    if let Some(next) = chars.next() {
                        if next == '#' {
                            note += 1;
                            consumed += 1;
                        } else if next == 'b' {
                            note = (note + 11) % 12; // 'b' is a flat, so we subtract 1 from the note
                            consumed += 1;
                        }
                    }
                    Ok((TerminalNote::Note { pitch: Pitch(octave, note) }, &input[consumed..]))
                } else {
                    Err(ScanError::Generic(
                        format!("Expected Note: note name {next} is not a valid note."),
                    ))
                }
            } else {
                Err(ScanError::Generic(
                    format!("Expected letter [a-g] after octave number after {first}"),
                ))
            }
        } else {
            Err(ScanError::Generic(
                "Expected Note: octave number or note letter".to_string(),
            ))
        }
    }
}

impl Scanner for DurationScanner {
    type Output = MusicTime;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // if it starts with '<', then scan a duration
        if let Some('<') = input.chars().next() {
            if let Some(end) = find_matching(&input[1..], '<', '>') {
                let duration = &input[1..=end];
                let rest = &input[end + 2..];
                if duration.contains('/') {
                    // it's a ratio
                    let mut parts = duration.split('/');
                    match (parts.next().and_then(|s| s.parse().ok()), parts.next().and_then(|s| s.parse().ok())) {
                        (Some(num), Some(denom)) => {
                            Ok((MusicTime(0, Beat::new(num, denom)), rest))
                        }
                        _ => {
                            eprintln!("Unable to parse {duration} as duration. Defaulting to 1");
                            Ok((MusicTime::beats(1), rest))
                        }
                    }
                } else {
                    let duration_int = duration.parse::<u32>().unwrap_or(0);
                    Ok((MusicTime::beats(duration_int), rest))
                }
            } else {
                Err(ScanError::Generic("Expected '>'".to_string()))
            }
        } else {
            Ok((MusicTime::beats(1), input))
        }
    }
}

impl Scanner for FractionScanner {
    type Output = Ratio<isize>;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // scan a fraction in the form of "num/denom"
        let mut parts = input.split('/');
        match (parts.next().and_then(|s| s.parse().ok()), parts.next().and_then(|s| s.parse().ok())) {
            (Some(num), Some(denom)) => {
                if denom == 0 {
                    Err(ScanError::Generic("Denominator cannot be zero".to_string()))
                } else {
                    Ok((Ratio::new(num, denom), ""))
                }
            }
            (Some(num), None) => {
                // if only numerator is provided, assume denominator is 1
                Ok((Ratio::new(num, 1), ""))
            }
            _ => Err(ScanError::Generic("Expected fraction in the form of num/denom".to_string())),
        }
    }
}

impl Scanner for MetaControlScanner {
    type Output = MetaControl;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        let mut chars = input.chars();
        if let Some(first) = chars.next() {
            if let Some('=') = chars.next() {
                let mut rest = &input[2..];
                match first {
                    'i' => {
                        let (instrument, new_input) = InstrumentScanner.scan(rest)?;
                        rest = new_input;
                        Ok((MetaControl::ChangeInstrument(instrument), rest))
                    }
                    'v' => {
                        let (volume, new_input) = VolumeScanner.scan(rest)?;
                        rest = new_input;
                        Ok((MetaControl::ChangeVolume(volume), rest))
                    }
                    _ => {
                        Err(ScanError::Generic(format!(
                            "Expected MetaControl: i= or v=, found {}=",
                            first
                        )))
                    }
                }
            } else {
                Err(ScanError::Generic(format!("Expected '=' to follow meta control character {first}")))
            }
        } else {
            Err(ScanError::Generic("Expected MetaControl".to_string()))
        }
    }
}

impl Scanner for NonTerminalScanner {
    type Output = String;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // scan [-a-zA-Z0-9/] and return largest prefix
        let other_allowed_chars: HashSet<char> = "-/#?".chars().collect();
        let is_nt_char = |c: char| c.is_alphabetic() || c.is_ascii_digit() ||
            other_allowed_chars.contains(&c);
        let mut chars = input.chars();
        if let Some(first) = chars.next() {
            if is_nt_char(first) {
                let mut non_terminal = first.to_string();
                while let Some(c) = chars.next() {
                    if is_nt_char(c) {
                        non_terminal.push(c);
                    } else {
                        // prepend chars with c
                        let rest = &input[non_terminal.len()..];
                        return Ok((non_terminal, rest));
                    }
                }
                Ok((non_terminal, chars.as_str()))
            } else {
                Err(ScanError::Generic(format!("Expected NonTerminal but got {first}")))
            }
        } else {
            Err(ScanError::Generic(format!("Expected NonTerminal, but it's an empty string")))
        }
    }
}

impl Scanner for InstrumentScanner {
    type Output = Instrument;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // scan instrument name
        let mut chars = input.chars();
        if let Some(first) = chars.next() {
            if first.is_alphabetic() {
                let mut instrument = first.to_string();
                while let Some(c) = chars.next() {
                    if c.is_alphanumeric() || c == '_' {
                        instrument.push(c);
                    } else {
                        return Ok((instrument.parse().unwrap(), chars.as_str()));
                    }
                }
                Ok((instrument.parse().unwrap(), chars.as_str()))
            } else {
                Err(ScanError::Generic("Expected Instrument".to_string()))
            }
        } else {
            Err(ScanError::Generic("Expected Instrument".to_string()))
        }
    }
}

impl Scanner for VolumeScanner {
    type Output = Volume;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        // scan volume value
        let mut chars = input.chars();
        if let Some(first) = chars.next() {
            if first.is_digit(10) {
                let mut volume = first.to_string();
                while let Some(c) = chars.next() {
                    if c.is_digit(10) {
                        volume.push(c);
                    } else {
                        return Ok((Volume(volume.parse().unwrap()), chars.as_str()));
                    }
                }
                Ok((Volume(volume.parse().unwrap()), chars.as_str()))
            } else {
                Err(ScanError::Generic("Expected Volume".to_string()))
            }
        } else {
            Err(ScanError::Generic("Expected Volume".to_string()))
        }
    }
}

/// Assume that exactly 1 opening char has already been found. Find the next closing char.
fn find_matching(input: &str, open: char, close: char) -> Option<usize> {
    let mut stack = 1;
    for (i, c) in input.chars().enumerate() {
        if c == open {
            stack += 1;
        } else if c == close {
            stack -= 1;
            if stack == 0 {
                return Some(i);
            }
        }
    }
    None
}

pub struct StringScanner(String);

impl Scanner for StringScanner {
    type Output = String;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        if input.starts_with(&self.0) {
            Ok((self.0.clone(), &input[self.0.len()..]))
        } else {
            Err(ScanError::Generic(format!("Expected string: {}", self.0)))
        }
    }
}

pub struct SpaceScanner;

impl Scanner for SpaceScanner {
    type Output = ();

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        let trimmed = input.trim_start();
        if trimmed.len() < input.len() {
            Ok(((), trimmed))
        } else {
            Err(ScanError::Generic("Expected space".to_string()))
        }
    }
}

pub struct ConcatScan<S, T>(S, T);
pub struct DisjointScan<S, T> {
    scanner_a: (ScanPrefix, S),
    scanner_b: (Option<ScanPrefix>, T),
}

pub struct KleeneScan<S>(S);

pub struct MapScanner<S, F> {
    scanner: S,
    mapper: F,
}

pub struct ConsumeScanner<S>(S);

pub struct MapInputScanner<S, F> {
    scanner: S,
    mapper: F,
}

pub fn trim<S>(scan: S) -> impl Scanner<Output=S::Output>
where
    S: Scanner,
{
    scan_map_input(scan, |s| s.trim_start().trim_end())
}

pub fn consume<S>(scan: S) -> impl Scanner<Output=S::Output>
where
    S: Scanner,
{
    ConsumeScanner(scan)
}

pub fn scan_map<S, F, T>(scan: S, map: F) -> impl Scanner<Output=T>
where
    S: Scanner,
    F: Fn(S::Output) -> T,
{
    MapScanner {
        scanner: scan,
        mapper: map,
    }
}

pub fn scan_map_input<S, F>(scan: S, map: F) -> impl Scanner<Output=S::Output>
where
    S: Scanner,
    F: Fn(&str) -> &str,
{
    MapInputScanner {
        scanner: scan,
        mapper: map,
    }
}

pub fn kleene<S>(scan: S) -> impl Scanner<Output=Vec<S::Output>>
where
    S: Scanner,
{
    KleeneScan(scan)
}

pub fn concat<S, T, U, V>(scan1: S, scan2: T) -> impl Scanner<Output=(U, V)>
where
    S: Scanner<Output=U>,
    T: Scanner<Output=V>,
{
    ConcatScan(scan1, scan2)
}

pub fn disjoint<S, T, U>(
    prefix1: ScanPrefix,
    scan1: S,
    prefix2: Option<ScanPrefix>,
    scan2: T,
) -> impl Scanner<Output=U>
where
    S: Scanner<Output=U>,
    T: Scanner<Output=U>,
{
    DisjointScan {
        scanner_a: (prefix1, scan1),
        scanner_b: (prefix2, scan2),
    }
}

impl<S, T, U, V> Scanner for ConcatScan<S, T>
where
    S: Scanner<Output=U>,
    T: Scanner<Output=V>,
{
    type Output = (U, V);

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        self.0
            .scan(input)
            .and_then(|(u, new_input)| self.1.scan(new_input).map(|(v, s)| ((u, v), s)))
    }
}

impl<S, T, U> Scanner for DisjointScan<S, T>
where
    S: Scanner<Output=U>,
    T: Scanner<Output=U>,
{
    type Output = U;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        if input.starts_with(&self.scanner_a.0) {
            self.scanner_a.1.scan(input)
        } else if let Some(prefix) = &self.scanner_b.0 {
            if input.starts_with(prefix) {
                self.scanner_b.1.scan(input)
            } else {
                Err(ScanError::ExpectedEither(
                    self.scanner_a.0.to_string(),
                    self.scanner_b
                        .0
                        .as_ref()
                        .map(|s| s.to_string())
                        .unwrap_or("Something else".to_string()),
                ))
            }
        } else {
            self.scanner_b.1.scan(input)
        }
    }
}

impl<S> Scanner for KleeneScan<S>
where
    S: Scanner,
{
    type Output = Vec<S::Output>;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        let mut results = Vec::new();
        let mut remaining_input = input;

        while let Ok((result, new_input)) = self.0.scan(remaining_input) {
            results.push(result);
            remaining_input = new_input;
        }

        Ok((results, remaining_input))
    }
}

impl<S, T, U> Scanner for MapScanner<S, T>
where
    S: Scanner,
    T: Fn(S::Output) -> U,
{
    type Output = U;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        self.scanner
            .scan(input)
            .map(|(output, new_input)| ((self.mapper)(output), new_input))
    }
}

impl<S> Scanner for ConsumeScanner<S>
where
    S: Scanner,
{
    type Output = S::Output;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        self.0.scan(input).and_then(|(output, new_input)| {
            if new_input.is_empty() {
                Ok((output, new_input))
            } else {
                Err(ScanError::Generic("Did not consume entire input".to_string()))
            }
        })
    }
}

impl<S, T> Scanner for MapInputScanner<S, T>
where
    S: Scanner,
    T: Fn(&str) -> &str,
{
    type Output = S::Output;

    fn scan<'a>(&self, input: &'a str) -> Result<(Self::Output, &'a str)> {
        self.scanner
            .scan((self.mapper)(input))
            .map(|(output, new_input)| (output, new_input))
    }
}


#[cfg(test)]
mod test {
    use num::rational::Ratio;
    use crate::cfg::scan::{consume, ConsumeScanner, DurationScanner, FractionScanner, GrammarScanner, InstrumentScanner, MetaControlScanner, MusicPrimitiveRepeatScanner, MusicPrimitiveScanner, MusicStringScanner, MusicTransformScanner, NonTerminalScanner, NoteScanner, ProductionScanner, Scanner, SymbolScanner, TerminalScanner, VolumeScanner};

    #[test]
    fn test_1() {
        let input = "start S\nS = [x3][:4c<1> :4d :_ :f# :g :c ::i=piano B]\nB = :0c";
        let scanner = consume(GrammarScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_instrument() {
        let input = "piano";
        let scanner = ConsumeScanner(InstrumentScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_duration() {
        let input = "<1/4>";
        let scanner = ConsumeScanner(DurationScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_fraction() {
        let input = "3/4";
        let scanner = consume(FractionScanner);
        let result = scanner.scan(input).unwrap().0;
        assert_eq!(result, Ratio::new(3isize, 4isize));
    }

    #[test]
    fn test_fraction_2() {
        let input = "-3/4";
        let scanner = consume(FractionScanner);
        let result = scanner.scan(input).unwrap().0;
        assert_eq!(result, Ratio::new(-3isize, 4isize));
    }

    #[test]
    fn test_fraction_3() {
        let input = "3";
        let scanner = consume(FractionScanner);
        let result = scanner.scan(input).unwrap().0;
        assert_eq!(result, Ratio::new(3isize, 1isize));
    }

    #[test]
    fn test_volume() {
        let input = "20";
        let scanner = ConsumeScanner(VolumeScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_note() {
        let input = "4c#";
        let scanner = ConsumeScanner(NoteScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rest() {
        let input = "_";
        let scanner = ConsumeScanner(NoteScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_meta_control() {
        let input = "i=piano";
        let scanner = ConsumeScanner(MetaControlScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_meta_control_terminal() {
        let input = ":i=piano";
        let scanner = ConsumeScanner(TerminalScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_terminal() {
        let input = "4c<1>";
        let scanner = ConsumeScanner(TerminalScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_nonterminal() {
        let input = "S-b";
        let scanner = ConsumeScanner(NonTerminalScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn symbol_scanner_1() {
        let input = ":bb";
        let scanner = ConsumeScanner(SymbolScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn symbol_scanner_2() {
        let input = "::i=piano";
        let scanner = ConsumeScanner(SymbolScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn symbol_scanner_3() {
        let input = "T";
        let scanner = ConsumeScanner(SymbolScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn symbol_scanner_4() {
        let input = "(";
        let scanner = ConsumeScanner(SymbolScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_err());
    }

    #[test]
    fn primitive_scanner_1() {
        let input = "(";
        let scanner = MusicPrimitiveScanner;
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_err());
    }

    #[test]
    fn primitive_scanner_2() {
        let input = "[x2][(]";
        let scanner = MusicPrimitiveRepeatScanner;
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_err());
    }

    #[test]
    fn music_string_scanner_0() {
        // without any repeats or splits so far
        let input = ":4c<1> :4d :_ :f# :g :c ::i=piano Ba-c";
        let scanner = ConsumeScanner(MusicStringScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_string_scanner_1() {
        // without any repeats or splits so far
        let input = ":4c<1> :4d :_ :f# :g :c ::i=piano B";
        let scanner = ConsumeScanner(MusicStringScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_transform_scanner_1() {
        let input = "x3";
        let scanner = ConsumeScanner(MusicTransformScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_transform_scanner_2() {
        let input = "T1";
        let scanner = ConsumeScanner(MusicTransformScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_transform_scanner_3() {
        let input = "T-1";
        let scanner = ConsumeScanner(MusicTransformScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_primitive_repeat_scanner() {
        let input = "[x3][:4c<1> :4d :_ :f# :g :c ::i=piano B]";
        let scanner = ConsumeScanner(MusicPrimitiveRepeatScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_primitive_repeat_bad_string_test_1() {
        let input = "[x3][nont( nont2]";
        let scanner = ConsumeScanner(MusicPrimitiveRepeatScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_err());
    }

    #[test]
    fn music_primitive_split_scanner() {
        let input = "{:4c<1> :4d :_ :f# :g :c ::i=piano B | :4c<1> :4d :_ :f# :g :c ::i=piano B }";
        let scanner = ConsumeScanner(MusicPrimitiveScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn music_string_scanner_2() {
        // with splits and repeats
        let input = "{:4c<1> :4d :_ :f# :g :c ::i=piano B | [x3][:4c<1> :4d :_ :f# :g :c ::i=piano B]}";
        let scanner = ConsumeScanner(MusicStringScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }

    #[test]
    fn production_scanner_1() {
        let input = "S = [x3][:4c<1> :4d :_ :f# :g :c ::i=piano B]";
        let scanner = ConsumeScanner(ProductionScanner);
        let result = scanner.scan(input);
        println!("result: {result:#?}");
        assert!(result.is_ok());
    }
}