// Per-player playback queue with repeat/shuffle support.

use serde::{Deserialize, Serialize};
use rand::seq::SliceRandom;

/// Track info needed for Squeeze streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: String,       // local file path
    pub duration: f64,      // seconds
    pub format: String,     // "flac", "mp3", etc.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    Off,
    One,
    All,
}

/// Playback queue for a single Squeeze player.
#[derive(Debug, Clone)]
pub struct PlayQueue {
    tracks: Vec<QueueTrack>,
    /// Indices into `tracks` defining play order.
    order: Vec<usize>,
    /// Current position within `order`.
    position: Option<usize>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

impl PlayQueue {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            order: Vec::new(),
            position: None,
            repeat: RepeatMode::Off,
            shuffle: false,
        }
    }

    /// Replace the entire queue and start at the given index.
    pub fn set_tracks(&mut self, tracks: Vec<QueueTrack>, start_index: usize) {
        let len = tracks.len();
        self.tracks = tracks;
        self.order = (0..len).collect();

        if self.shuffle && len > 1 {
            self.reshuffle_around(start_index);
        }

        self.position = if len > 0 {
            if self.shuffle {
                Some(0) // start_index is at position 0 after reshuffle
            } else {
                Some(start_index.min(len - 1))
            }
        } else {
            None
        };
    }

    /// Get the currently playing track.
    pub fn current(&self) -> Option<&QueueTrack> {
        let pos = self.position?;
        let idx = *self.order.get(pos)?;
        self.tracks.get(idx)
    }

    /// Get the current position index in the order.
    pub fn current_position(&self) -> Option<usize> {
        self.position
    }

    /// Peek at the next track without advancing.
    pub fn peek_next(&self) -> Option<&QueueTrack> {
        let pos = self.position?;
        let next_pos = match self.repeat {
            RepeatMode::One => pos,
            RepeatMode::All => {
                if pos + 1 >= self.order.len() {
                    0
                } else {
                    pos + 1
                }
            }
            RepeatMode::Off => {
                if pos + 1 >= self.order.len() {
                    return None;
                }
                pos + 1
            }
        };
        let idx = *self.order.get(next_pos)?;
        self.tracks.get(idx)
    }

    /// Advance to the next track. Returns the new current track.
    pub fn next(&mut self) -> Option<&QueueTrack> {
        let pos = self.position?;
        match self.repeat {
            RepeatMode::One => {
                // Stay on same track
            }
            RepeatMode::All => {
                if pos + 1 >= self.order.len() {
                    // Wrap around — reshuffle if needed
                    if self.shuffle {
                        self.reshuffle_around(usize::MAX); // no anchor
                    }
                    self.position = Some(0);
                } else {
                    self.position = Some(pos + 1);
                }
            }
            RepeatMode::Off => {
                if pos + 1 >= self.order.len() {
                    self.position = None;
                    return None;
                }
                self.position = Some(pos + 1);
            }
        }
        self.current()
    }

    /// Go to previous track.
    pub fn previous(&mut self) -> Option<&QueueTrack> {
        let pos = self.position?;
        if pos == 0 {
            match self.repeat {
                RepeatMode::All => {
                    self.position = Some(self.order.len().saturating_sub(1));
                }
                _ => {
                    // Stay at beginning
                }
            }
        } else {
            self.position = Some(pos - 1);
        }
        self.current()
    }

    /// Set shuffle mode. Reshuffles order if enabling.
    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle == enabled {
            return;
        }
        self.shuffle = enabled;
        if enabled {
            let current_idx = self.position.and_then(|p| self.order.get(p).copied());
            if let Some(idx) = current_idx {
                self.reshuffle_around(idx);
                self.position = Some(0);
            }
        } else {
            // Restore natural order, keep current track
            let current_idx = self.position.and_then(|p| self.order.get(p).copied());
            self.order = (0..self.tracks.len()).collect();
            if let Some(idx) = current_idx {
                self.position = self.order.iter().position(|&i| i == idx);
            }
        }
    }

    /// Reshuffle placing `anchor_idx` at position 0.
    fn reshuffle_around(&mut self, anchor_idx: usize) {
        let mut rng = rand::rng();
        let len = self.tracks.len();
        if len <= 1 {
            return;
        }

        self.order = (0..len).collect();

        if anchor_idx < len {
            // Remove anchor, shuffle rest, prepend anchor
            self.order.retain(|&i| i != anchor_idx);
            self.order.shuffle(&mut rng);
            self.order.insert(0, anchor_idx);
        } else {
            self.order.shuffle(&mut rng);
        }
    }

    /// Get all tracks in current play order.
    pub fn ordered_tracks(&self) -> Vec<&QueueTrack> {
        self.order
            .iter()
            .filter_map(|&idx| self.tracks.get(idx))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Find a track by its ID and return a clone.
    pub fn find_track_by_id(&self, id: i64) -> Option<QueueTrack> {
        self.tracks.iter().find(|t| t.id == id).cloned()
    }
}
