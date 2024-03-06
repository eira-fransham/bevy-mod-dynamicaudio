use std::sync::Arc;

use crate::sink::SpatialSink;
use bevy::{
    audio::{DefaultSpatialScale, PlaybackMode},
    ecs::system::SystemParam,
    prelude::*,
};
use fundsp::prelude::*;
use rodio::Sink;

use rodio::{
    dynamic_mixer::{self, DynamicMixerController},
    OutputStream, OutputStreamHandle, Sample, Source,
};

/// Set the target for an [`AudioSource`][crate::AudioSource]. Audio produced by this source will be rerouted
/// to the designated stream instead of the global stream.
#[derive(Component)]
pub struct AudioTarget {
    /// The handle to the output stream, which will be used to send audio to the mixer.
    pub target: Entity,
}

#[derive(Component)]
pub struct AudioProcessorSource<S, Node>
where
    Node: AudioNode,
{
    source: S,
    node: Node,
    buffer_in: Buffer<Node::Sample>,
    buffer_out: Buffer<Node::Sample>,
    buffer_idx: usize,
    buffer_channel: usize,
}

impl<S, N> AudioProcessorSource<S, N>
where
    S: Source<Item = N::Sample>,
    N: AudioNode,
    N::Sample: Default + Clone + rodio::Sample + 'static,
{
    fn new(source: S, node: N) -> Self {
        let mut buffer_in = Buffer::<N::Sample>::with_channels(source.channels() as usize);

        for buf in buffer_in.vec_mut() {
            *buf = vec![Default::default(); fundsp::MAX_BUFFER_SIZE];
        }

        let mut buffer_out = Buffer::<N::Sample>::with_channels(source.channels() as usize);

        for buf in buffer_out.vec_mut() {
            *buf = vec![Default::default(); fundsp::MAX_BUFFER_SIZE];
        }

        let mut out = Self {
            source,
            node,
            buffer_in,
            buffer_out,
            buffer_idx: 0,
            buffer_channel: 0,
        };

        out.fill_buf();

        out
    }

    fn fill_buf(&mut self) {
        let chans = self.channels();
        let buf = self.buffer_in.self_mut();
        for i in 0..fundsp::MAX_BUFFER_SIZE {
            for chan in 0..chans {
                buf[chan as usize][i] = self.source.next().unwrap_or_default();
            }
        }

        self.node.process(
            fundsp::MAX_BUFFER_SIZE,
            self.buffer_in.self_ref(),
            self.buffer_out.self_mut(),
        );
    }
}

impl<S, N> Iterator for AudioProcessorSource<S, N>
where
    S: Source<Item = N::Sample>,
    N: AudioNode,
    N::Sample: rodio::Sample + 'static,
{
    type Item = N::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.buffer_out.at(self.buffer_channel).get(self.buffer_idx) {
            self.buffer_channel += 1;
            if self.buffer_channel >= self.channels() as usize {
                self.buffer_idx += 1;
                self.buffer_channel = 0;
            }

            return Some(sample.clone());
        }

        self.buffer_idx = 0;
        self.buffer_channel = 0;

        self.fill_buf();

        self.next()
    }
}

impl<S, N> Source for AudioProcessorSource<S, N>
where
    S: Source<Item = N::Sample>,
    N: AudioNode,
    N::Sample: rodio::Sample + 'static,
{
    fn current_frame_len(&self) -> Option<usize> {
        Some(fundsp::MAX_BUFFER_SIZE)
    }

    fn channels(&self) -> u16 {
        self.source.channels() as _
    }

    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.source.total_duration()
    }
}

pub struct Global;

#[derive(Component)]
pub struct Mixer<Node> {
    pub processor: Option<Node>,
}

#[derive(Component)]
pub struct MixerController {
    controller: Arc<DynamicMixerController<f32>>,
}

#[derive(Component)]
pub struct SpatialAudioSink {
    sink: SpatialSink,
}

#[derive(Component)]
pub struct AudioSink {
    sink: Sink,
}

impl SpatialAudioSink {
    /// Set the two ears position.
    pub fn set_ears_position(&self, left_position: Vec3, right_position: Vec3) {
        self.sink.set_left_ear_position(left_position.to_array());
        self.sink.set_right_ear_position(right_position.to_array());
    }

    /// Set the listener position, with an ear on each side separated by `gap`.
    pub fn set_listener_position(&self, position: Transform, gap: f32) {
        self.set_ears_position(
            position.translation + position.left() * gap / 2.0,
            position.translation + position.right() * gap / 2.0,
        );
    }

    /// Set the emitter position.
    pub fn set_emitter_position(&self, position: Vec3) {
        self.sink.set_emitter_position(position.to_array());
    }
}

impl AudioSinkPlayback for SpatialAudioSink {
    fn volume(&self) -> f32 {
        self.sink.volume()
    }

    fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    fn speed(&self) -> f32 {
        self.sink.speed()
    }

    fn set_speed(&self, speed: f32) {
        self.sink.set_speed(speed);
    }

    fn play(&self) {
        self.sink.play();
    }

    fn pause(&self) {
        self.sink.pause();
    }

    fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    fn stop(&self) {
        self.sink.stop();
    }

    fn empty(&self) -> bool {
        self.sink.empty()
    }
}

impl AudioSinkPlayback for AudioSink {
    fn volume(&self) -> f32 {
        self.sink.volume()
    }

    fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    fn speed(&self) -> f32 {
        self.sink.speed()
    }

    fn set_speed(&self, speed: f32) {
        self.sink.set_speed(speed);
    }

    fn play(&self) {
        self.sink.play();
    }

    fn pause(&self) {
        self.sink.pause();
    }

    fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    fn stop(&self) {
        self.sink.stop();
    }

    fn empty(&self) -> bool {
        self.sink.empty()
    }
}

#[derive(Resource)]
pub struct AudioOutput {
    stream_handle: Option<OutputStreamHandle>,
}

pub struct AudioOutputMainStorage {
    _stream: Option<OutputStream>,
}

impl AudioOutput {
    pub fn new() -> (Self, AudioOutputMainStorage) {
        match OutputStream::try_default() {
            Ok((stream, stream_handle)) => (
                Self {
                    stream_handle: Some(stream_handle),
                },
                AudioOutputMainStorage {
                    _stream: Some(stream),
                },
            ),
            Err(e) => {
                warn!("No audio device found: {}", e);
                (
                    Self {
                        stream_handle: None,
                    },
                    AudioOutputMainStorage { _stream: None },
                )
            }
        }
    }
}

impl Default for AudioOutput {
    fn default() -> Self {
        if let Ok((stream, stream_handle)) = OutputStream::try_default() {
            // We leak `OutputStream` to prevent the audio from stopping.
            std::mem::forget(stream);
            Self {
                stream_handle: Some(stream_handle),
            }
        } else {
            warn!("No audio device found.");
            Self {
                stream_handle: None,
            }
        }
    }
}

/// Marker for internal use, to despawn entities when playback finishes.
#[derive(Component)]
pub struct PlaybackDespawnMarker;

/// Marker for internal use, to remove audio components when playback finishes.
#[derive(Component)]
pub struct PlaybackRemoveMarker;

#[derive(SystemParam)]
pub(crate) struct EarPositions<'w, 's> {
    pub(crate) query: Query<'w, 's, (Entity, &'static GlobalTransform, &'static SpatialListener)>,
}
impl<'w, 's> EarPositions<'w, 's> {
    /// Gets a set of transformed ear positions.
    ///
    /// If there are no listeners, use the default values. If a user has added multiple
    /// listeners for whatever reason, we will return the first value.
    pub(crate) fn get(&self) -> (Vec3, Vec3) {
        let (left_ear, right_ear) = self
            .query
            .iter()
            .next()
            .map(|(_, transform, settings)| {
                (
                    transform.transform_point(settings.left_ear_offset),
                    transform.transform_point(settings.right_ear_offset),
                )
            })
            .unwrap_or_else(|| {
                let settings = SpatialListener::default();
                (settings.left_ear_offset, settings.right_ear_offset)
            });

        (left_ear, right_ear)
    }

    pub(crate) fn multiple_listeners(&self) -> bool {
        self.query.iter().len() > 1
    }
}

pub(crate) fn play_queued_audio_system<Src: Asset + Decodable>(
    audio_output: Res<AudioOutput>,
    audio_sources: Res<Assets<Src>>,
    global_volume: Res<GlobalVolume>,
    mut query_nonplaying: Query<
        (
            Entity,
            &Handle<Src>,
            Option<&AudioTarget>,
            &PlaybackSettings,
            Option<&GlobalTransform>,
        ),
        (Without<AudioSink>, Without<SpatialAudioSink>),
    >,
    query_mixers: Query<&MixerController>,
    ear_positions: EarPositions,
    default_spatial_scale: Res<DefaultSpatialScale>,
    mut commands: Commands,
) where
    f32: cpal::FromSample<Src::DecoderItem>,
    Src::Decoder: Send + Sync + 'static,
{
    let Some(global_stream_handle) = audio_output.stream_handle.as_ref() else {
        return;
    };

    for (entity, source_handle, target, settings, maybe_emitter_transform) in
        query_nonplaying.iter_mut()
    {
        let Some(audio_source) = audio_sources.get(source_handle) else {
            warn!("Could not find audio source: {:?}", source_handle);
            continue;
        };

        let output_target = target
            .as_ref()
            .and_then(|target| query_mixers.get(target.target).ok())
            .map(Ok)
            .unwrap_or(Err(global_stream_handle));

        let mut commands = commands.entity(entity);

        let source = match settings.mode {
            PlaybackMode::Loop => Err(audio_source.decoder().repeat_infinite()),
            PlaybackMode::Once => Ok(audio_source.decoder()),
            PlaybackMode::Despawn => {
                commands.insert(PlaybackDespawnMarker);
                Ok(audio_source.decoder())
            }
            PlaybackMode::Remove => {
                commands.insert(PlaybackRemoveMarker);
                Ok(audio_source.decoder())
            }
        };

        // audio data is available (has loaded), begin playback and insert sink component
        if settings.spatial {
            let (left_ear, right_ear) = ear_positions.get();

            // We can only use one `SpatialListener`. If there are more than that, then
            // the user may have made a mistake.
            if ear_positions.multiple_listeners() {
                warn!(
                    "Multiple SpatialListeners found. Using {:?}.",
                    ear_positions.query.iter().next().unwrap().0
                );
            }

            let scale = settings.spatial_scale.unwrap_or(default_spatial_scale.0).0;

            let emitter_translation = if let Some(emitter_transform) = maybe_emitter_transform {
                (emitter_transform.translation() * scale).into()
            } else {
                warn!("Spatial AudioBundle with no GlobalTransform component. Using zero.");
                Vec3::ZERO.into()
            };

            let (sink, output) = SpatialSink::new_idle(
                emitter_translation,
                (left_ear * scale).into(),
                (right_ear * scale).into(),
            );

            match source {
                Ok(src) => sink.append(src),
                Err(src) => sink.append(src),
            }

            sink.set_speed(settings.speed);
            sink.set_volume(settings.volume.get() * global_volume.volume.get());

            if settings.paused {
                sink.pause();
            }

            commands.insert(SpatialAudioSink { sink });

            match output_target {
                Ok(mixer) => mixer.controller.add(output),
                Err(handle) => {
                    if let Err(e) = handle.play_raw(output) {
                        warn!("Couldn't play sound: {}", e);
                    }
                }
            }
        } else {
            let (sink, output) = Sink::new_idle();

            match source {
                Ok(src) => sink.append(src),
                Err(src) => sink.append(src),
            }

            sink.set_speed(settings.speed);
            sink.set_volume(settings.volume.get() * global_volume.volume.get());

            if settings.paused {
                sink.pause();
            }

            commands.insert(AudioSink { sink });

            match output_target {
                Ok(mixer) => mixer.controller.add(output),
                Err(handle) => {
                    if let Err(e) = handle.play_raw(output) {
                        warn!("Couldn't play sound: {}", e);
                    }
                }
            }
        }
    }
}

/// Run Condition to only play audio if the audio output is available
pub fn audio_output_available(audio_output: Res<AudioOutput>) -> bool {
    audio_output.stream_handle.is_some()
}

/// Updates spatial audio sinks when emitter positions change.
pub fn update_emitter_positions(
    mut emitters: Query<
        (&GlobalTransform, &SpatialAudioSink, &PlaybackSettings),
        Or<(Changed<GlobalTransform>, Changed<PlaybackSettings>)>,
    >,
    default_spatial_scale: Res<DefaultSpatialScale>,
) {
    for (transform, sink, settings) in emitters.iter_mut() {
        let scale = settings.spatial_scale.unwrap_or(default_spatial_scale.0).0;

        let translation = transform.translation() * scale;
        sink.set_emitter_position(translation);
    }
}

/// Updates spatial audio sink ear positions when spatial listeners change.
pub(crate) fn update_listener_positions(
    mut emitters: Query<(&SpatialAudioSink, &PlaybackSettings)>,
    changed_listener: Query<
        (),
        (
            Or<(
                Changed<SpatialListener>,
                Changed<GlobalTransform>,
                Changed<PlaybackSettings>,
            )>,
            With<SpatialListener>,
        ),
    >,
    ear_positions: EarPositions,
    default_spatial_scale: Res<DefaultSpatialScale>,
) {
    if !default_spatial_scale.is_changed() && changed_listener.is_empty() {
        return;
    }

    let (left_ear, right_ear) = ear_positions.get();

    for (sink, settings) in emitters.iter_mut() {
        let scale = settings.spatial_scale.unwrap_or(default_spatial_scale.0).0;

        sink.set_ears_position(left_ear * scale, right_ear * scale);
    }
}

pub(crate) fn create_mixers<Node>(
    audio_output: Res<AudioOutput>,
    mut query_nonplaying: Query<
        (Entity, &mut Mixer<Node>, Option<&AudioTarget>),
        Without<MixerController>,
    >,
    query_mixers: Query<&MixerController>,
    mut commands: Commands,
) where
    f32: cpal::FromSample<Node::Sample>,
    Node: AudioNode + Send + Sync + 'static,
    Node::Sample: Sample + cpal::FromSample<f32> + Send + Sync + 'static,
{
    let Some(global_stream_handle) = audio_output.stream_handle.as_ref() else {
        return;
    };

    for (entity, mut mixer, target) in query_nonplaying.iter_mut() {
        let output_target = target
            .as_ref()
            .and_then(|target| query_mixers.get(target.target).ok())
            .map(Ok)
            .unwrap_or(Err(global_stream_handle));

        let (controller, mixer_out) = dynamic_mixer::mixer::<f32>(2, 44100);

        let processor =
            AudioProcessorSource::new(mixer_out.convert_samples(), mixer.processor.take().unwrap());

        let output = processor;

        commands
            .entity(entity)
            .insert(MixerController { controller });

        match output_target {
            Ok(mixer) => mixer.controller.add(output.convert_samples()),
            Err(handle) => {
                if let Err(e) = handle.play_raw(output.convert_samples()) {
                    warn!("Couldn't play output of mixer: {}", e);
                }
            }
        }
    }
}
