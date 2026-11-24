//! Segment table: how a file is split between connections.
//!
//! This is the heart of the acceleration:
//! 1. The file is split into N equal parts, one per connection.
//! 2. When a connection finishes its part early, it does not sit idle:
//!    it takes the part with the most bytes still remaining and splits it
//!    in half, then downloads the second half itself (dynamic segmentation).
//!
//! `end` is exclusive. A segment is finished when `pos >= end`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub pos: u64,
    pub end: u64,
    /// A connection is currently working on this segment (runtime only).
    #[serde(skip)]
    pub active: bool,
    /// While it downloads: how far the file is surely on the disk (Windows
    /// keeps what was written in memory for a while; after a power cut that
    /// part of the file is empty). The list saves this, not `pos`: the few
    /// seconds after it are downloaded again instead of left as a hole.
    /// None = all of `pos` is on the disk (runtime only).
    #[serde(skip)]
    pub on_disk: Option<u64>,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Segment { start, pos: start, end, active: false, on_disk: None }
    }
    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.pos)
    }
    pub fn done(&self) -> u64 {
        self.pos.saturating_sub(self.start)
    }
    pub fn is_finished(&self) -> bool {
        self.pos >= self.end
    }
}

/// Split `total` bytes into at most `connections` parts, never making a part
/// smaller than `min_size` (except when the whole file is smaller).
pub fn plan(total: u64, connections: u32, min_size: u64) -> Vec<Segment> {
    if total == 0 {
        return vec![Segment::new(0, 0)];
    }
    let min_size = min_size.max(1);
    let by_size = (total / min_size).max(1);
    let n = (connections.max(1) as u64).min(by_size);
    let part = total / n;
    (0..n)
        .map(|i| {
            let start = i * part;
            let end = if i == n - 1 { total } else { (i + 1) * part };
            Segment::new(start, end)
        })
        .collect()
}

/// Total bytes already downloaded across all segments.
pub fn downloaded(segs: &[Segment]) -> u64 {
    segs.iter().map(|s| s.done()).sum()
}

/// The table as the list saves it: each part only as far as it is surely on
/// the disk.
pub fn as_on_disk(segs: &[Segment]) -> Vec<Segment> {
    segs.iter()
        .map(|s| {
            let mut s = s.clone();
            if let Some(d) = s.on_disk {
                s.pos = d.clamp(s.start, s.pos);
            }
            s.on_disk = None;
            s
        })
        .collect()
}

pub fn all_finished(segs: &[Segment]) -> bool {
    segs.iter().all(|s| s.is_finished())
}

/// Give a free connection something to do. First it takes any unfinished
/// segment nobody is working on (e.g. after resume); otherwise it splits the
/// active segment with the most remaining bytes. Returns the segment index
/// the caller now owns (already marked active), or `None` when there is
/// nothing worth taking.
pub fn claim_work(segs: &mut Vec<Segment>, min_split: u64) -> Option<usize> {
    if let Some(i) = segs.iter().position(|s| !s.active && !s.is_finished()) {
        segs[i].active = true;
        return Some(i);
    }
    split_largest(segs, min_split)
}

/// Split the active segment with the largest remaining range in half.
/// Only splits when both halves would be at least `min_split` bytes.
pub fn split_largest(segs: &mut Vec<Segment>, min_split: u64) -> Option<usize> {
    let min_split = min_split.max(1);
    let (idx, rem) = segs
        .iter()
        .enumerate()
        .filter(|(_, s)| s.active && !s.is_finished())
        .map(|(i, s)| (i, s.remaining()))
        .max_by_key(|(_, r)| *r)?;
    if rem < min_split * 2 {
        return None;
    }
    let mid = segs[idx].pos + rem / 2;
    let old_end = segs[idx].end;
    segs[idx].end = mid;
    let mut new_seg = Segment::new(mid, old_end);
    new_seg.active = true;
    // nothing of it written yet, so nothing of it on the disk
    new_seg.on_disk = segs[idx].on_disk.map(|_| mid);
    segs.push(new_seg);
    Some(segs.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_covers_whole_file() {
        let segs = plan(1000, 8, 10);
        assert_eq!(segs.len(), 8);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs.last().unwrap().end, 1000);
        for w in segs.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
    }

    #[test]
    fn plan_respects_min_size() {
        let segs = plan(1000, 8, 400);
        assert_eq!(segs.len(), 2);
        let segs = plan(100, 8, 400);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].end, 100);
    }

    #[test]
    fn claim_prefers_idle_segments() {
        let mut segs = plan(1000, 4, 10);
        let a = claim_work(&mut segs, 10).unwrap();
        let b = claim_work(&mut segs, 10).unwrap();
        assert_ne!(a, b);
        assert!(segs[a].active && segs[b].active);
    }

    #[test]
    fn dynamic_split_takes_largest_remaining() {
        let mut segs = plan(1000, 2, 10);
        for s in segs.iter_mut() {
            s.active = true;
        }
        // first segment finished, second only 100 bytes in
        segs[0].pos = segs[0].end;
        segs[0].active = false;
        segs[1].pos = 600;
        let i = claim_work(&mut segs, 10).unwrap();
        assert_eq!(i, 2);
        assert_eq!(segs[1].end, 800);
        assert_eq!(segs[2].start, 800);
        assert_eq!(segs[2].end, 1000);
        // ranges still cover everything without overlap
        assert_eq!(segs[1].remaining() + segs[2].remaining(), 400);
    }

    #[test]
    fn no_split_when_too_small() {
        let mut segs = vec![Segment::new(0, 100)];
        segs[0].active = true;
        segs[0].pos = 90;
        assert!(claim_work(&mut segs, 10).is_none());
    }

    #[test]
    fn saved_only_as_far_as_on_the_disk() {
        let mut segs = plan(1000, 2, 10);
        for s in segs.iter_mut() {
            s.on_disk = Some(s.pos);
            s.active = true;
        }
        segs[0].pos = 300;
        segs[0].on_disk = Some(200);
        segs[1].pos = 700;
        let i = split_largest(&mut segs, 10).unwrap();
        assert_eq!(segs[i].on_disk, Some(segs[i].start), "a new part has nothing on the disk");
        let saved = as_on_disk(&segs);
        assert_eq!(saved[0].pos, 200);
        assert_eq!(saved[1].pos, 500, "written but not on the disk yet: again");
        assert!(saved.iter().all(|s| s.on_disk.is_none()));
        // no download running: everything counts
        let idle = vec![Segment { pos: 50, ..Segment::new(0, 100) }];
        assert_eq!(as_on_disk(&idle)[0].pos, 50);
    }

    #[test]
    fn downloaded_sum() {
        let mut segs = plan(1000, 4, 10);
        segs[0].pos += 50;
        segs[3].pos += 25;
        assert_eq!(downloaded(&segs), 75);
        assert!(!all_finished(&segs));
    }
}
