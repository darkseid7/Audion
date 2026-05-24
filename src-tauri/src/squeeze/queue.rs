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

    /// Insert tracks at a specific position in the queue without changing the current track.
    pub fn insert_tracks(&mut self, tracks: Vec<QueueTrack>, position: usize) {
        let insert_at = position.min(self.tracks.len());
        let count = tracks.len();

        // Insert the new tracks into the tracks vec
        for (i, track) in tracks.into_iter().enumerate() {
            self.tracks.insert(insert_at + i, track);
        }

        // Update order indices: shift any index >= insert_at by count
        for idx in self.order.iter_mut() {
            if *idx >= insert_at {
                *idx += count;
            }
        }

        // Add new track indices to order (after current position)
        let insert_order_pos = if let Some(pos) = self.position {
            pos + 1
        } else {
            self.order.len()
        };
        for i in 0..count {
            let order_pos = (insert_order_pos + i).min(self.order.len());
            self.order.insert(order_pos, insert_at + i);
        }
    }

    /// Replace the queue tracks and order without changing the current playback position.
    /// Used when the frontend reorders the queue.
    pub fn replace_queue_keep_current(&mut self, tracks: Vec<QueueTrack>, current_track_id: i64) {
        let len = tracks.len();
        self.tracks = tracks;
        self.order = (0..len).collect();

        // Find where the currently playing track is in the new queue
        self.position = self.tracks.iter().position(|t| t.id == current_track_id);

        if self.shuffle && len > 1 {
            if let Some(pos) = self.position {
                let current_idx = self.order[pos];
                self.reshuffle_around(current_idx);
                self.position = Some(0);
            }
        }
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

    /// Jump to an absolute position in the queue.
    pub fn jump_to(&mut self, index: usize) {
        if index < self.order.len() {
            self.position = Some(index);
        }
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
