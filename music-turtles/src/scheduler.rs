use std::time::Duration;
use rodio::Source;
use rodio::source::SineWave;
use crate::composition::{Composition, Frequency, Instrument, Pitch, Track, Volume};
use crate::player::{AtomicSound, Playable};
use crate::time::{MusicTime, Seconds, TimeSignature, BPM};

pub type Cursor = MusicTime;

pub struct Scheduler {
    pub bpm: BPM,
    pub time_signature: TimeSignature,
    pub tracks: Vec<(Track, Cursor)>,
    pub lookahead: MusicTime,
    pub looped: bool,
    pub loop_time: MusicTime,
}

#[derive(Debug, PartialOrd, PartialEq)]
pub struct ScheduledSound {
    time: Seconds,
    duration: Seconds,
    volume: Volume,
    instrument: Instrument,
    pitch: Pitch
}

pub fn get_sine_source(length: Seconds, frequency: Frequency) -> impl Source<Item=f32> {
    let sources: Vec<Box<dyn Source<Item=f32> + Send>> = vec![
        Box::new(
            SineWave::new(frequency)
                .take_duration(Duration::from_secs_f32(length))
                .fade_in(Duration::from_millis(40))
        ),
        Box::new(
            SineWave::new(frequency).fade_out(Duration::from_millis(40))
        )
    ];

    rodio::source::from_iter(sources)
        .amplify((3.0 * 44.0 / frequency).clamp(0.0, 1.0))
}

impl Playable for ScheduledSound {
    /// start time, duration, and actual sound
    fn get_source(&self) -> (Seconds, Seconds, Box<dyn Source<Item=f32> + Send + 'static>) {
        let source = get_sine_source(self.duration, self.pitch.to_frequency());
        (
            self.time,
            self.duration,
            Box::new(source)
        )
    }
}

impl From<ScheduledSound> for AtomicSound {
    fn from(value: ScheduledSound) -> Self {
        AtomicSound {
            start: value.time,
            duration: value.duration,
            volume: value.volume,
            pitch: value.pitch,
            instrument: value.instrument,
        }
    }
}

impl Scheduler {

    pub fn set_composition(&mut self, composition: Composition) {
        self.time_signature = composition.time_signature;
        self.tracks = composition.tracks.into_iter()
            .map(|t| (t, MusicTime::zero()))
            .collect();
    }
    
    pub fn ended(&self) -> bool {
        self.tracks.iter()
            .filter_map(|(t, cursor)| 
                t.get_end(self.time_signature)
                    .map(|end| *cursor > end)
            ).all(|b| b)
    }

    /// get the next events and update the cursors if necessary
    pub fn get_next_events_and_update(&mut self, current_track_pos: Seconds) -> Vec<ScheduledSound> {
        let mut current_music_time = MusicTime::from_seconds(self.time_signature, self.bpm, current_track_pos);
        let loop_end = self.loop_time;
        while self.looped && current_music_time > loop_end {
            current_music_time = current_music_time.with(self.time_signature) - loop_end;
        }
        let loop_time_s = self.loop_time.to_seconds(self.time_signature, self.bpm);
        let mut end_music_time = current_music_time.with(self.time_signature) + self.lookahead;
        let end_non_looped = end_music_time;
        let looping = if self.looped && end_music_time > loop_end {
            while end_music_time > loop_end {
                end_music_time = end_music_time.with(self.time_signature) - loop_end;
            }
            true
        } else {
            false
        };
        let mut sounds = self.tracks.iter_mut()
            .flat_map(|(track, cursor)| {
                let be_exclusive = false; // *cursor != MusicTime::zero();
                let events = if looping {
                    // if end_non_looped < *cursor {
                    //     vec![]
                    // } else
                    // if *cursor <= end_music_time {
                    //     track.get_events_starting_between(*cursor, end_music_time, be_exclusive)
                    // } else {
                        let mut to_end = track.get_events_starting_between(*cursor, loop_end, be_exclusive);
                        let from_beg = track.get_events_starting_between(MusicTime::zero(), end_music_time, false);
                        to_end.extend(from_beg);
                        to_end
                    // }
                } else {
                    track.get_events_starting_between(*cursor, end_music_time, be_exclusive)
                };
                *cursor = end_music_time;
                // make sure looped sounds happen afterward
                events.into_iter()
                    .map(|e| {
                        let start = e.start.to_seconds(self.time_signature, self.bpm);
                        let duration = e.duration.as_music_time(self.time_signature).to_seconds(self.time_signature, self.bpm) * 0.9;
                        let volume = e.volume;
                        let instrument = track.instrument;
                        ScheduledSound {
                            time: start,
                            duration,
                            volume,
                            instrument,
                            pitch: e.pitch,
                        }
                    })
                    .map(|mut se| {
                        if self.looped {
                            while se.time < current_track_pos {
                                se.time += loop_time_s;
                            }
                        }
                        se
                    }).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        sounds.sort_by(|a: &ScheduledSound, b: &ScheduledSound| a.partial_cmp(b).unwrap());
        sounds
    }
}

#[cfg(test)]
mod test {
    use crate::composition::{Composition, Event, Instrument, Pitch, Track, TrackId, Volume};
    use crate::scheduler::{ScheduledSound, Scheduler};
    use crate::time::{Beat, Measure, MusicTime, Seconds, TimeSignature};

    fn comp_template(events: Vec<Event>) -> Composition {
        Composition {
            tracks: vec![
                Track {
                    identifier: TrackId::Custom(0),
                    instrument: Instrument::SineWave,
                    events,
                    rests: vec![],
                }
            ],
            time_signature: TimeSignature::common(),
        }
    }

    fn simulate_play_collect_events(
        mut scheduler: Scheduler,
        duration: Seconds,
        interval: Seconds,
    ) -> Vec<ScheduledSound> {
        let mut emitted_sounds = vec![];
        for i in (0..(duration / interval) as u64) {
            let elapsed = i as Seconds * interval;
            let sounds = scheduler.get_next_events_and_update(elapsed);
            emitted_sounds.extend(sounds);
        }
        emitted_sounds
    }
    #[test]
    fn test_scheduler_1() {
        let comp = comp_template(vec![
            Event {
                start: MusicTime(0, Beat::whole(0)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 0),
            },
            Event {
                start: MusicTime(0, Beat::whole(1)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 1),
            },
            Event {
                start: MusicTime(0, Beat::whole(2)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 2),
            },
            Event {
                start: MusicTime(0, Beat::whole(3)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 3),
            }
        ]);
        let mut scheduler = Scheduler {
            bpm: 120.0,
            time_signature: TimeSignature::common(),
            tracks: vec![],
            lookahead: MusicTime::measures(1),
            looped: false,
            loop_time: MusicTime::measures(4),
        };
        scheduler.set_composition(comp);
        let sounds = simulate_play_collect_events(scheduler, 5.0, 0.05);
        assert_eq!(sounds.len(), 4);
        assert_eq!(sounds.iter().map(|s| s.pitch).collect::<Vec<_>>(),
                   vec![Pitch(4, 0), Pitch(4, 1), Pitch(4, 2), Pitch(4, 3)]);
    }
    #[test]
    fn test_scheduler_2() {
        let comp = comp_template(vec![
            Event {
                start: MusicTime(0, Beat::whole(0)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 0),
            },
            Event {
                start: MusicTime(0, Beat::whole(3)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 3),
            },
            Event {
                start: MusicTime(0, Beat::whole(2)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 2),
            },
            Event {
                start: MusicTime(0, Beat::whole(1)),
                duration: Beat::whole(1),
                volume: Volume(100),
                pitch: Pitch(4, 1),
            }
        ]);
        let mut scheduler = Scheduler {
            bpm: 120.0,
            time_signature: TimeSignature::common(),
            tracks: vec![],
            lookahead: MusicTime::measures(1),
            looped: false,
            loop_time: MusicTime::measures(4),
        };
        scheduler.set_composition(comp);
        let sounds = simulate_play_collect_events(scheduler, 5.0, 0.05);
        assert_eq!(sounds.len(), 4);
        assert_eq!(sounds.iter().map(|s| s.pitch).collect::<Vec<_>>(),
                   vec![Pitch(4, 0), Pitch(4, 1), Pitch(4, 2), Pitch(4, 3)]);
    }
}