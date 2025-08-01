use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use midly::live::LiveEvent;
use midly::MidiMessage;
use rodio::{OutputStream, OutputStreamHandle, Source};
use crate::composition::{Event, Instrument, Pitch, Volume};
use crate::constants::get_fuzzy_mapping;
use crate::time::Seconds;

pub type MidiChannel = u8;

pub struct AtomicSound {
    pub start: Seconds,
    pub duration: Seconds,
    pub volume: Volume,
    pub pitch: Pitch,
    pub instrument: Instrument
}

pub trait AudioPlayer {
    fn play(&mut self, event: AtomicSound);

    fn play_from_ordered_channel<T: Into<AtomicSound>>(&mut self, queue: Receiver<T>) {
        let start_time = SystemTime::now();
        let mut end = start_time;
        for event in queue {
            let event = event.into();
            let current_time = SystemTime::now();
            let elapsed = current_time.duration_since(start_time).unwrap().as_secs_f32();
            let wait_time = event.start - elapsed;
            if wait_time > 0. {
                thread::sleep(Duration::from_secs_f32(wait_time));
            }
            end = SystemTime::max(end, current_time + Duration::from_secs_f32(f32::max(wait_time, 0.) + event.duration));
            // println!("playing sound: {start:?}");
            self.play(event);
        }
        // wait for the last sound to finish
        let wait_time = end.duration_since(SystemTime::now()).unwrap_or(Duration::from_secs(1)).as_secs_f32();
        if wait_time > 0. {
            thread::sleep(Duration::from_secs_f32(wait_time));
        }
    }
}

pub struct Player {
    stream: OutputStream,
    output_stream: OutputStreamHandle
}

pub trait Playable {
    /// get start time, duration, and actual sound
    fn get_source(&self) -> (Seconds, Seconds, Box<dyn Source<Item=f32> + Send + 'static>);
}

impl Player {
    pub fn new() -> Self {
        let (stream, output_stream) = OutputStream::try_default().unwrap();
        Player { stream, output_stream }
    }
    pub fn play(&self, source: impl Source<Item=f32> + Send + 'static) {
        let sink = rodio::Sink::try_new(&self.output_stream).unwrap();
        // thread::spawn(move || {
        //     let source: Box<dyn Source<Item=f32> + Send> = Box::new(source);
        //     sink.append(source);
        //     sink.sleep_until_end();
        // });
        let source: Box<dyn Source<Item=f32> + Send> = Box::new(source);
        sink.append(source);
        sink.detach();
    }

    /// Incoming events MUST BE IN ORDER
    pub fn play_from_ordered_channel<T: Playable>(&self, queue: Receiver<T>) {
        let start_pause = 0.1; // seconds
        let start_time = SystemTime::now() - std::time::Duration::from_secs_f32(start_pause);
        let mut end = start_time;
        for event in queue {
            let (start, duration, source) = event.get_source();
            let current_time = SystemTime::now();
            let elapsed = current_time.duration_since(start_time).unwrap().as_secs_f32();
            let wait_time = start - elapsed;
            // println!("Waiting for {wait_time} until {start}... (sound is {duration}s long)");
            if wait_time > 0. {
                thread::sleep(std::time::Duration::from_secs_f32(wait_time));
            }
            end = SystemTime::max(end, current_time + std::time::Duration::from_secs_f32(f32::max(wait_time, 0.) + duration));
            println!("playing sound: {start:?}");
            self.play(source);
        }
        // wait for the last sound to finish
        let wait_time = end.duration_since(SystemTime::now()).unwrap_or(Duration::from_secs(1)).as_secs_f32();
        if wait_time > 0. {
            std::thread::sleep(std::time::Duration::from_secs_f32(wait_time));
        }
    }
}

pub type MidiPort = u8;
pub struct MidiPlayer {
    name: String,
    port_channel_mapping: HashMap<Instrument, (MidiPort, MidiChannel)>,
    instrument_mapping: HashMap<Instrument, u8>,
    conn: Arc<HashMap<MidiPort, Mutex<midir::MidiOutputConnection>>>,
}

impl MidiPlayer {
    /// Create a new player with a name and a mapping. Mapping may be empty.
    pub fn new(name: String, port_channel_mapping: HashMap<Instrument, (MidiPort, MidiChannel)>) -> Result<Self, Box<dyn std::error::Error>> {
        let midi_out = midir::MidiOutput::new(&name)?;
        let out_ports = midi_out.ports();
        println!("Available output ports:");
        let mut conns = HashMap::new();
        for (i, p) in out_ports.iter().enumerate() {
            println!("{}: {} : {}", i, midi_out.port_name(p)?, p.id());
            let port = p;
            let midi_out_i = midir::MidiOutput::new(&format!("{}-{}", name, i))?;
            let conn = midi_out_i.connect(port, &format!("midir-connection-{i}"))?;
            conns.insert(i as MidiPort, Mutex::new(conn));
        }
        // // Pick a port
        // let port = &out_ports[0];
        // let conn = midi_out.connect(port, "midir-connection")?;
        // let conn = Arc::new(Mutex::new(conn));
        // conns.insert(0, Mutex::new(midi_out.connect(&out_ports[0], "music-turtles")?));
        println!("Created {} connections", conns.len());
        Ok(MidiPlayer { name, port_channel_mapping, conn: Arc::new(conns), instrument_mapping: get_fuzzy_mapping() })
    }

    pub fn get_port_channel(&self, instrument: Instrument) -> Option<(MidiPort, MidiChannel)> {
        self.port_channel_mapping.get(&instrument).cloned()
    }
}

impl AudioPlayer for MidiPlayer {
    fn play(&mut self, event: AtomicSound) {
        let note = event.pitch.to_midi_note();
        let volume = ((event.volume.0 as f32 / 100.) * 128.) as u8;
        let (port, channel) = self.get_port_channel(event.instrument)
            .unwrap();
        info!("Playing instrument {:?} on port {} channel {} at volume {}", event.instrument, port, channel, volume);
        let note_on_message = |channel: u8, key: u8, vol: u8| {
            let ev = LiveEvent::Midi {
                channel: channel.into(),
                message: MidiMessage::NoteOn {
                    key: key.into(),
                    vel: vol.into(),
                },
            };
            let mut buf = Vec::new();
            ev.write(&mut buf).unwrap();
            buf
        };
        let note_off_message = |channel: u8, key: u8, vol: u8| {
            let ev = LiveEvent::Midi {
                channel: channel.into(),
                message: MidiMessage::NoteOff {
                    key: key.into(),
                    vel: vol.into(),
                },
            };
            let mut buf = Vec::new();
            ev.write(&mut buf).unwrap();
            buf
        };
        let arc = Arc::clone(&self.conn);
        let thread_conn = Arc::clone(&self.conn);
        let mut conn = arc.get(&port).unwrap().lock()
            .unwrap();
        conn.send(&note_on_message(channel, note, volume)).unwrap();
        let duration = event.duration;
        thread::spawn(move || {
            thread::sleep(Duration::from_secs_f32(duration));
            let mut conn = thread_conn.get(&port).unwrap().lock().unwrap();
            conn.send(&note_off_message(channel, note, volume)).unwrap();
        });
    }
}