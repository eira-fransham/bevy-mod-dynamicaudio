//! Queue that plays sounds one after the other.

use std::{
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use parking_lot::RwLock;

use rodio::{source::Source, Sample};

/// Builds a new queue. It consists of an input and an output.
///
/// The input can be used to add sounds to the end of the queue, while the output implements
/// `Source` and plays the sounds.
///
/// The parameter indicates how the queue should behave if the queue becomes empty:
///
/// - If you pass `true`, then the queue is infinite and will play a silence instead until you add
///   a new sound.
/// - If you pass `false`, then the queue will report that it has finished playing.
///
pub fn queue<Src>(
    keep_alive_if_empty: bool,
    initial_source: Src,
) -> (Arc<SourcesQueueInput<Src>>, SourcesQueueOutput<Src>) {
    let input = Arc::new(SourcesQueueInput {
        keep_alive_if_empty: AtomicBool::new(keep_alive_if_empty),
        current: RwLock::new(initial_source).into(),
    });

    let output = SourcesQueueOutput {
        input: input.clone(),
    };

    (input, output)
}

// TODO: consider reimplementing this with `from_factory`

/// The input of the queue.
pub struct SourcesQueueInput<Src> {
    keep_alive_if_empty: AtomicBool,
    current: RwLock<Src>,
}

impl<Src> SourcesQueueInput<Src> {
    /// Adds a new source to the end of the queue.
    #[inline]
    pub fn set_source<T>(&self, source: T) -> Src
    where
        T: Into<Src>,
    {
        mem::replace(&mut *self.current.write(), source.into())
    }

    /// Sets whether the queue stays alive if there's no more sound to play.
    ///
    /// See also the constructor.
    pub fn set_keep_alive_if_empty(&self, keep_alive_if_empty: bool) {
        self.keep_alive_if_empty
            .store(keep_alive_if_empty, Ordering::Release);
    }
}

pub struct SourcesQueueOutput<Src> {
    input: Arc<SourcesQueueInput<Src>>,
}

const THRESHOLD: usize = 512;
impl<S> Source for SourcesQueueOutput<S>
where
    S: Source,
    S::Item: Sample,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        // This function is non-trivial because the boundary between two sounds in the queue should
        // be a frame boundary as well.
        //
        // The current sound is free to return `None` for `current_frame_len()`, in which case
        // we *should* return the number of samples remaining the current sound.
        // This can be estimated with `size_hint()`.
        //
        // If the `size_hint` is `None` as well, we are in the worst case scenario. To handle this
        // situation we force a frame to have a maximum number of samples indicate by this
        // constant.

        // Try the current `current_frame_len`.
        if let Some(val) = self.input.current.read().current_frame_len() {
            if val != 0 {
                return Some(val);
            } else if self.input.keep_alive_if_empty.load(Ordering::Acquire) {
                // The next source will be a filler silence which will have the length of `THRESHOLD`
                return Some(THRESHOLD);
            }
        }

        // Try the size hint.
        let (lower_bound, _) = self.input.current.read().size_hint();
        // The iterator default implementation just returns 0.
        // That's a problematic value, so skip it.
        if lower_bound > 0 {
            Some(lower_bound)
        } else {
            // Otherwise we use the constant value.
            Some(THRESHOLD)
        }
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.input.current.read().channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.input.current.read().sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl<S> Iterator for SourcesQueueOutput<S>
where
    S: Source,
    S::Item: Sample,
{
    type Item = S::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(next) = self.input.current.write().next() {
            Some(next)
        } else {
            if self.input.keep_alive_if_empty.load(Ordering::Acquire) {
                Some(S::Item::zero_value())
            } else {
                None
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.input.current.read().size_hint().0, None)
    }
}
