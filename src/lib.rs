use audio::{
    audio_output_available, create_mixers, play_queued_audio_system, update_emitter_positions,
    update_listener_positions, AudioOutput,
};
use bevy::{
    audio::{AudioLoader, DefaultSpatialScale, PlaybackMode, SpatialScale, Volume},
    ecs::schedule::SystemSet,
    prelude::*,
    transform::TransformSystem,
};
use fundsp::audionode::AudioNode;
use rodio::Sample;

pub mod audio;
pub mod queue;
pub mod sink;

/// Set for the audio playback systems, so they can share a run condition
#[derive(SystemSet, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
struct AudioPlaySet;

/// Adds support for audio playback to a Bevy Application
///
/// Insert an [`AudioBundle`] onto your entities to play audio.
#[derive(Default)]
pub struct AudioPlugin {
    /// The global volume for all audio entities.
    pub global_volume: GlobalVolume,
    /// The scale factor applied to the positions of audio sources and listeners for
    /// spatial audio.
    pub default_spatial_scale: SpatialScale,
}

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        let (output_handle, output) = AudioOutput::new();
        app.register_type::<Volume>()
            .register_type::<GlobalVolume>()
            .register_type::<SpatialListener>()
            .register_type::<DefaultSpatialScale>()
            .register_type::<PlaybackMode>()
            .register_type::<PlaybackSettings>()
            .insert_resource(self.global_volume)
            .insert_resource(DefaultSpatialScale(self.default_spatial_scale))
            .configure_sets(
                PostUpdate,
                AudioPlaySet
                    .run_if(audio_output_available)
                    .after(TransformSystem::TransformPropagate), // For spatial audio transforms
            )
            .insert_non_send_resource(output)
            .insert_resource(output_handle)
            .add_systems(
                PostUpdate,
                (update_emitter_positions, update_listener_positions).in_set(AudioPlaySet),
            );

        app.add_audio_source::<AudioSource>();
        app.init_asset_loader::<AudioLoader>();

        app.add_audio_source::<Pitch>();
    }
}

pub trait AddAudioSource {
    fn add_audio_source<T>(&mut self) -> &mut Self
    where
        T: Decodable + Asset,
        T::Decoder: Sync,
        f32: rodio::cpal::FromSample<T::DecoderItem>;
}

impl AddAudioSource for App {
    fn add_audio_source<T>(&mut self) -> &mut Self
    where
        T: Decodable + Asset,
        T::Decoder: Sync,
        f32: rodio::cpal::FromSample<T::DecoderItem>,
    {
        self.init_asset::<T>().add_systems(
            PostUpdate,
            // (play_queued_audio_system::<T>, cleanup_finished_audio::<T>).in_set(AudioPlaySet),
            play_queued_audio_system::<T>.in_set(AudioPlaySet),
        );
        self
    }
}

pub trait AddAudioMixer {
    fn add_audio_mixer<Node>(&mut self) -> &mut Self
    where
        f32: cpal::FromSample<Node::Sample>,
        Node: AudioNode + Send + Sync + 'static,
        Node::Sample: Sample + cpal::FromSample<f32> + Send + Sync + 'static;
}

impl AddAudioMixer for App {
    fn add_audio_mixer<Node>(&mut self) -> &mut Self
    where
        f32: cpal::FromSample<Node::Sample>,
        Node: AudioNode + Send + Sync + 'static,
        Node::Sample: Sample + cpal::FromSample<f32> + Send + Sync + 'static,
    {
        self.add_systems(PostUpdate, create_mixers::<Node>.in_set(AudioPlaySet));
        self
    }
}
