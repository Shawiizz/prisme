//! The audio thread allocates nothing, the software mixer and its live inputs at work. Alone in
//! its test binary: the allocator counts every thread while armed.

mod common;

use common::{constant_track, Rig};
use prisme_engine::{Command, InputChannels, LiveSource, OutputRouting};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn the_live_mixer_allocates_nothing_on_the_audio_thread() {
    let mut rig = Rig::with_channels(8, 6);
    rig.set_input_level(0, 0.3);
    rig.set_input_level(4, 0.2);
    rig.set_input_level(6, 0.1);
    let routing = OutputRouting { booth: Some(4), ..OutputRouting::default() };
    let mic = Some(InputChannels { first: 4, stereo: false });
    let aux = Some(InputChannels { first: 6, stereo: true });
    let commands = [
        Command::SetRouting { routing },
        Command::LoadTrack { deck: 0, track: constant_track(10.0, 0.5) },
        Command::LoadTrack { deck: 1, track: constant_track(10.0, 0.25) },
        Command::SetPlaying { deck: 0, playing: true },
        Command::SetPlaying { deck: 1, playing: true },
        Command::SetLiveInput { source: LiveSource::Mic, channels: mic },
        Command::SetLiveInput { source: LiveSource::Aux, channels: aux },
        Command::SetLiveOn { source: LiveSource::Mic, on: true },
        Command::SetLiveOn { source: LiveSource::Aux, on: true },
        Command::SetLiveCue { source: LiveSource::Mic, cue: true },
        Command::SetLiveLevel { source: LiveSource::Aux, gain: 0.5 },
        Command::SetTalkover { enabled: true, depth_db: -12.0 },
        Command::SetBalance { balance: -0.3 },
        Command::SetBoothGain { gain: 0.7 },
        Command::SetCueMix { mix: 0.5 },
        Command::SetPassthrough { deck: 1, enabled: true },
    ];
    // once through before counting: the rig's own buffers settle
    rig.run(0.05);
    for command in commands {
        rig.send(command);
    }
    ARMED.store(true, Ordering::SeqCst);
    rig.run(0.5);
    rig.send(Command::SetPassthrough { deck: 1, enabled: false });
    rig.send(Command::SetLiveOn { source: LiveSource::Mic, on: false });
    rig.run(0.5);
    ARMED.store(false, Ordering::SeqCst);
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);
    assert!(rig.output_frame().iter().any(|s| s.abs() > 0.05), "it played");
}
