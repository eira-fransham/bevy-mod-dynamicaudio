use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use crate::queue;

use rodio::{
    source::{Amplify, Pausable, PeriodicAccess, Skippable, Spatial, Speed, Stoppable},
    OutputStreamHandle, PlayError, Sample, Source,
};

type SinkSourceInner<Src> = Stoppable<Skippable<Amplify<Pausable<Speed<Src>>>>>;
type SinkSource<Src> = PeriodicAccess<SinkSourceInner<Src>, AccessControlsFn<Src>>;
type AccessControlsFn<Src> = Box<dyn FnMut(&mut SinkSourceInner<Src>) + Send + Sync + 'static>;
type SpatialAccessControlsFn<Src> = Box<dyn FnMut(&mut Spatial<Src>) + Send + Sync + 'static>;
type SpatialSinkSource<Src> = PeriodicAccess<Spatial<Src>, SpatialAccessControlsFn<Src>>;

/// Handle to a device that outputs sounds.
///
/// Dropping the `Sink` stops all sounds. You can use `detach` if you want the sounds to continue
/// playing.
pub struct Sink<Src> {
    queue_tx: Arc<queue::SourcesQueueInput<SinkSource<Src>>>,
    controls: Arc<Controls>,
    detached: bool,
}

struct Controls {
    pause: AtomicBool,
    volume: Mutex<f32>,
    stopped: AtomicBool,
    speed: Mutex<f32>,
    to_clear: Mutex<u32>,
}

fn to_sink_source<Src>(controls: Arc<Controls>, source: Src) -> SinkSource<Src>
where
    Src: Source,
    Src::Item: Sample,
{
    source
        .speed(1.0)
        .pausable(false)
        .amplify(1.0)
        .skippable()
        .stoppable()
        .periodic_access(
            Duration::from_millis(5),
            Box::new(move |src| {
                if controls.stopped.load(Ordering::SeqCst) {
                    src.stop();
                }
                {
                    let mut to_clear = controls.to_clear.lock().unwrap();
                    if *to_clear > 0 {
                        let _ = src.inner_mut().skip();
                        *to_clear -= 1;
                    }
                }
                let amp = src.inner_mut().inner_mut();
                amp.set_factor(*controls.volume.lock().unwrap());
                amp.inner_mut()
                    .set_paused(controls.pause.load(Ordering::SeqCst));
                amp.inner_mut()
                    .inner_mut()
                    .set_factor(*controls.speed.lock().unwrap());
            }),
        )
}

impl<Src> Sink<Src>
where
    Src: Source + Send + Sync + 'static,
    Src::Item: Sample,
    f32: cpal::FromSample<Src::Item>,
{
    /// Builds a new `Sink`, beginning playback on a stream.
    #[inline]
    pub fn try_new(stream: &OutputStreamHandle, source: Src) -> Result<Self, PlayError> {
        let (sink, queue_rx) = Sink::new_idle(source);
        stream.play_raw(queue_rx.convert_samples())?;
        Ok(sink)
    }
}

impl<Src> Sink<Src>
where
    Src: Source,
    Src::Item: Sample,
{
    /// Builds a new `Sink`.
    #[inline]
    pub fn new_idle(source: Src) -> (Self, queue::SourcesQueueOutput<SinkSource<Src>>) {
        let controls = Arc::new(Controls {
            pause: AtomicBool::new(false),
            volume: Mutex::new(1.0),
            stopped: AtomicBool::new(false),
            speed: Mutex::new(1.0),
            to_clear: Mutex::new(0),
        });
        let (queue_tx, queue_rx) = queue::queue(false, to_sink_source(controls.clone(), source));

        let sink = Sink {
            queue_tx,
            controls,
            detached: false,
        };
        (sink, queue_rx)
    }

    /// Appends a sound to the queue of sounds to play.
    #[inline]
    pub fn set_source<T>(&self, source: T) -> SinkSource<Src>
    where
        T: Into<Src>,
    {
        if self.controls.stopped.load(Ordering::SeqCst) {
            self.controls.stopped.store(false, Ordering::SeqCst);
        }

        let controls = self.controls.clone();

        let source: Src = source.into();
        let source = to_sink_source(controls, source);
        self.queue_tx.set_source(source)
    }

    /// Gets the volume of the sound.
    ///
    /// The value `1.0` is the "normal" volume (unfiltered input). Any value other than 1.0 will
    /// multiply each sample by this value.
    #[inline]
    pub fn volume(&self) -> f32 {
        *self.controls.volume.lock().unwrap()
    }

    /// Changes the volume of the sound.
    ///
    /// The value `1.0` is the "normal" volume (unfiltered input). Any value other than `1.0` will
    /// multiply each sample by this value.
    #[inline]
    pub fn set_volume(&self, value: f32) {
        *self.controls.volume.lock().unwrap() = value;
    }

    /// Gets the speed of the sound.
    ///
    /// The value `1.0` is the "normal" speed (unfiltered input). Any value other than `1.0` will
    /// change the play speed of the sound.
    #[inline]
    pub fn speed(&self) -> f32 {
        *self.controls.speed.lock().unwrap()
    }

    /// Changes the speed of the sound.
    ///
    /// The value `1.0` is the "normal" speed (unfiltered input). Any value other than `1.0` will
    /// change the play speed of the sound.
    #[inline]
    pub fn set_speed(&self, value: f32) {
        *self.controls.speed.lock().unwrap() = value;
    }

    /// Resumes playback of a paused sink.
    ///
    /// No effect if not paused.
    #[inline]
    pub fn play(&self) {
        self.controls.pause.store(false, Ordering::SeqCst);
    }

    /// Pauses playback of this sink.
    ///
    /// No effect if already paused.
    ///
    /// A paused sink can be resumed with `play()`.
    pub fn pause(&self) {
        self.controls.pause.store(true, Ordering::SeqCst);
    }

    /// Gets if a sink is paused
    ///
    /// Sinks can be paused and resumed using `pause()` and `play()`. This returns `true` if the
    /// sink is paused.
    pub fn is_paused(&self) -> bool {
        self.controls.pause.load(Ordering::SeqCst)
    }

    /// Stops the sink by emptying the queue.
    #[inline]
    pub fn stop(&self) {
        self.controls.stopped.store(true, Ordering::SeqCst);
    }

    /// Destroys the sink without stopping the sounds that are still playing.
    #[inline]
    pub fn detach(mut self) {
        self.detached = true;
    }
}

impl<Src> Drop for Sink<Src> {
    #[inline]
    fn drop(&mut self) {
        self.queue_tx.set_keep_alive_if_empty(false);

        if !self.detached {
            self.controls.stopped.store(true, Ordering::Relaxed);
        }
    }
}

pub struct SpatialSink<Src>
where
    Src: Source,
    Src::Item: Sample,
{
    sink: Sink<SpatialSinkSource<Src>>,
    positions: Arc<Mutex<SoundPositions>>,
}

struct SoundPositions {
    emitter_position: [f32; 3],
    left_ear: [f32; 3],
    right_ear: [f32; 3],
}

fn to_spatial_sink_source<Src>(
    positions: Arc<Mutex<SoundPositions>>,
    source: Src,
) -> SpatialSinkSource<Src>
where
    Src: Source,
    Src::Item: Sample,
{
    let spatial = {
        let pos_lock = positions.lock().unwrap();
        Spatial::new(
            source,
            pos_lock.emitter_position,
            pos_lock.left_ear,
            pos_lock.right_ear,
        )
    };
    spatial.periodic_access(
        Duration::from_millis(10),
        Box::new(move |i| {
            let pos = positions.lock().unwrap();
            i.set_positions(pos.emitter_position, pos.left_ear, pos.right_ear);
        }),
    )
}
impl<Src> SpatialSink<Src>
where
    Src: Source + Send + Sync + 'static,
    Src::Item: Sample + Send + Sync + 'static,
    f32: cpal::FromSample<Src::Item>,
{
    /// Builds a new `SpatialSink`.
    pub fn try_new(
        stream: &OutputStreamHandle,
        emitter_position: [f32; 3],
        left_ear: [f32; 3],
        right_ear: [f32; 3],
        source: Src,
    ) -> Result<Self, PlayError> {
        let positions = Arc::new(Mutex::new(SoundPositions {
            emitter_position,
            left_ear,
            right_ear,
        }));
        let (sink, queue_rx) = Sink::new_idle(to_spatial_sink_source(positions.clone(), source));
        stream.play_raw(queue_rx.convert_samples())?;
        Ok(SpatialSink { sink, positions })
    }
}

impl<Src> SpatialSink<Src>
where
    Src: Source,
    Src::Item: Sample,
{
    /// Builds a new `Sink`.
    #[inline]
    pub fn new_idle(
        emitter_position: [f32; 3],
        left_ear: [f32; 3],
        right_ear: [f32; 3],
        source: Src,
    ) -> (
        Self,
        queue::SourcesQueueOutput<SinkSource<SpatialSinkSource<Src>>>,
    ) {
        let positions = Arc::new(Mutex::new(SoundPositions {
            emitter_position,
            left_ear,
            right_ear,
        }));
        let (sink, out) = Sink::new_idle(to_spatial_sink_source(positions.clone(), source));
        (SpatialSink { sink, positions }, out)
    }

    /// Sets the position of the sound emitter in 3 dimensional space.
    pub fn set_emitter_position(&self, pos: [f32; 3]) {
        self.positions.lock().unwrap().emitter_position = pos;
    }

    /// Sets the position of the left ear in 3 dimensional space.
    pub fn set_left_ear_position(&self, pos: [f32; 3]) {
        self.positions.lock().unwrap().left_ear = pos;
    }

    /// Sets the position of the right ear in 3 dimensional space.
    pub fn set_right_ear_position(&self, pos: [f32; 3]) {
        self.positions.lock().unwrap().right_ear = pos;
    }

    /// Appends a sound to the queue of sounds to play.
    #[inline]
    pub fn set_source<T>(&self, source: T) -> SinkSource<SpatialSinkSource<Src>>
    where
        T: Into<Src>,
    {
        let positions = self.positions.clone();
        let source = to_spatial_sink_source(positions.clone(), source.into());
        self.sink.set_source(source)
    }

    // Gets the volume of the sound.
    ///
    /// The value `1.0` is the "normal" volume (unfiltered input). Any value other than 1.0 will
    /// multiply each sample by this value.
    #[inline]
    pub fn volume(&self) -> f32 {
        self.sink.volume()
    }

    /// Changes the volume of the sound.
    ///
    /// The value `1.0` is the "normal" volume (unfiltered input). Any value other than 1.0 will
    /// multiply each sample by this value.
    #[inline]
    pub fn set_volume(&self, value: f32) {
        self.sink.set_volume(value);
    }

    /// Gets the speed of the sound.
    ///
    /// The value `1.0` is the "normal" speed (unfiltered input). Any value other than `1.0` will
    /// change the play speed of the sound.
    #[inline]
    pub fn speed(&self) -> f32 {
        self.sink.speed()
    }

    /// Changes the speed of the sound.
    ///
    /// The value `1.0` is the "normal" speed (unfiltered input). Any value other than `1.0` will
    /// change the play speed of the sound.
    #[inline]
    pub fn set_speed(&self, value: f32) {
        self.sink.set_speed(value)
    }

    /// Resumes playback of a paused sound.
    ///
    /// No effect if not paused.
    #[inline]
    pub fn play(&self) {
        self.sink.play();
    }

    /// Pauses playback of this sink.
    ///
    /// No effect if already paused.
    ///
    /// A paused sound can be resumed with `play()`.
    pub fn pause(&self) {
        self.sink.pause();
    }

    /// Gets if a sound is paused
    ///
    /// Sounds can be paused and resumed using pause() and play(). This gets if a sound is paused.
    pub fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    /// Stops the sink by emptying the queue.
    #[inline]
    pub fn stop(&self) {
        self.sink.stop()
    }

    /// Destroys the sink without stopping the sounds that are still playing.
    #[inline]
    pub fn detach(self) {
        self.sink.detach();
    }
}
