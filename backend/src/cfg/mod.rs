pub mod scan;
pub mod interactive;

use crate::cfg::scan::Scanner;
use crate::cfg::scan::{consume, MusicStringScanner, ScanError};
use crate::composition::{Composition, Event, Instrument, Pitch, Track, TrackId, Volume};
use crate::time::{BeatUnit, MusicTime, TimeSignature};
use std::collections::HashMap;
use std::str::FromStr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grammar {
    start: NonTerminal,
    productions: Vec<Production>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Production(NonTerminal, MusicString);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicString(pub Vec<MusicPrimitive>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MusicPrimitive {
    Simple(Symbol),
    Split {
        branches: Vec<MusicString>
    },
    Repeat {
        num: usize,
        content: MusicString,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Symbol {
    NT(NonTerminal),
    T(Terminal),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NonTerminal {
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Terminal {
    Music {
        duration: MusicTime,
        note: TerminalNote,
    },
    Meta(MetaControl),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TerminalNote {
    Note {
        pitch: Pitch
    },
    Rest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MetaControl {
    ChangeInstrument(Instrument),
    ChangeVolume(Volume),
}

impl MusicString {
    pub fn compose(&self, time_signature: TimeSignature) -> Composition {
        let mut tracks = HashMap::new();
        fn add_event(tracks: &mut HashMap<Instrument, Track>, e: Event, instrument: Instrument) {
            if let Some(mut track) = tracks.get_mut(&instrument) {
                track.events.push(e);
            } else {
                tracks.insert(
                    instrument,
                    Track {
                        identifier: TrackId::Instrument(instrument),
                        instrument,
                        events: vec![e],
                    },
                );
            }
        }
        fn add_track(tracks: &mut HashMap<Instrument, Track>, track: Track) {
            if let Some(mtrack) = tracks.remove(&track.instrument) {
                tracks.insert(mtrack.instrument, mtrack + track);
            } else {
                tracks.insert(track.instrument, track);
            }
        }
        fn add_composition(tracks: &mut HashMap<Instrument, Track>, composition: Composition) {
            for track in composition.tracks {
                add_track(tracks, track);
            }
        }
        let mut current_mt = MusicTime::zero();
        let mut current_instrument = Instrument::SineWave;
        let mut current_volume = Volume(50);
        for mp in self.0.iter() {
            let duration = match mp {
                MusicPrimitive::Simple(sym) => match sym {
                    Symbol::NT(_) => MusicTime::zero(),
                    Symbol::T(Terminal::Music { note, duration }) => match note {
                        TerminalNote::Note { pitch } => {
                            add_event(
                                &mut tracks,
                                Event {
                                    start: current_mt,
                                    duration: duration.with(time_signature).total_beats(),
                                    volume: current_volume,
                                    pitch: *pitch,
                                },
                                current_instrument,
                            );
                            *duration
                        }
                        TerminalNote::Rest => *duration,
                    },
                    Symbol::T(Terminal::Meta(control)) => {
                        match control {
                            MetaControl::ChangeInstrument(i) => {
                                current_instrument = *i;
                            }
                            MetaControl::ChangeVolume(v) => {
                                current_volume = *v;
                            }
                        }
                        MusicTime::zero()
                    }
                },
                MusicPrimitive::Split { branches } => {
                    let comps: Vec<_> = branches
                        .into_iter()
                        .map(|ms| ms.compose(time_signature))
                        .map(|mut c| {
                            c.shift_by(current_mt);
                            c
                        })
                        .map(|c| (c.get_duration(), c))
                        .collect();
                    let uniform_duration = match comps.first() {
                        Some((duration, _c)) => {
                            if comps.iter().all(|(d, _c)| d == duration) {
                                Some(*duration)
                            } else {
                                None
                            }
                        }
                        // there are none, so yes they are
                        None => None,
                    };
                    if let Some(dur) = uniform_duration {
                        for (_d, comp) in comps {
                            add_composition(&mut tracks, comp);
                        }
                        dur
                    } else {
                        panic!("Not all split tracks have the same duration: {:?}",
                               comps.iter().map(|(d, c)| d).collect::<Vec<_>>()
                        )
                    }
                }
                MusicPrimitive::Repeat { content, num } => {
                    let composed = content.compose(time_signature);
                    let duration = composed.get_duration();
                    let mut offset = MusicTime::zero();
                    for _i in 0..*num {
                        let mut comp_i = composed.clone();
                        comp_i.shift_by(offset);
                        add_composition(&mut tracks, comp_i);
                        offset = offset.with(time_signature) + duration;
                    }
                    let mut total_duration = MusicTime::zero();
                    for _i in 0..*num {
                        total_duration = total_duration.with(time_signature) + duration;
                    }
                    total_duration
                }
            };
            current_mt = current_mt.with(time_signature) + duration;
        }
        Composition {
            tracks: tracks.into_values().collect(),
            time_signature,
        }
    }
}

impl FromStr for MusicString {
    type Err = ScanError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let scanner = consume(MusicStringScanner);
        scanner.scan(s).map(|(r, _s)| r)
    }
}
