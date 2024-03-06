# `bevy-mod-dynamicaudio`

This is an experimental replacement for the built-in audio handling in [Bevy](https://bevyengine.org/).
Specifically, it allows heirarchical "mixers", which are generic audio processors - currently
programmed using the [fundsp](https://github.com/SamiPerttu/fundsp) API but a better long-term
solution would be to have a custom API that just requires that a type can be built from a `rodio::Source`
plus some settings. This basic pattern of having a trait for creating types with an associated type for
settings is common in Bevy.

### TODO: Show video example
