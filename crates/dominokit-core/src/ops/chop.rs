//! Chopper: cut every note at a regular grid that starts at tick 0 of the
//! clip (the start of the copied range) and runs every `step` ticks, in every
//! track. A note crossing grid lines becomes one note per grid cell it
//! sounds in, with the same key and velocity. Notes that cross no grid line,
//! events, settings and track lengths are untouched.
//!
//! The step is an exact fraction of a tick so musical divisions that don't
//! divide the PPQ evenly (e.g. an eighth triplet at PPQ 100 = 33⅓) don't
//! drift: grid line `i` is at `round_half_up(i × step)`.

use thiserror::Error;

use rayon::prelude::*;

use crate::format::clip::{Clip, NoteItem, TopNode, Track, TrackChild};


#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChopError {
    #[error("enter a chop length in ticks, e.g. 120 or 100/3")]
    Invalid,
    #[error("the chop length must be at least 1 tick")]
    TooShort,
    #[error("not enough memory for {0} notes (about {1} MB)")]
    OutOfMemory(u64, u64),
}

/// Grid spacing in ticks, as a reduced fraction `numer / denom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChopStep {
    numer: u64,
    denom: u64,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

impl ChopStep {
    /// `numer / denom` ticks; at least one tick.
    pub fn new(numer: u64, denom: u64) -> Result<ChopStep, ChopError> {
        if denom == 0 || numer == 0 {
            return Err(ChopError::Invalid);
        }
        let g = gcd(numer, denom);
        let step = ChopStep { numer: numer / g, denom: denom / g };
        if step.numer < step.denom {
            return Err(ChopError::TooShort);
        }
        Ok(step)
    }

    pub fn ticks(n: u32) -> Result<ChopStep, ChopError> {
        ChopStep::new(n as u64, 1)
    }

    /// Parse `120`, `100/3` or `33.5`.
    pub fn parse(text: &str) -> Result<ChopStep, ChopError> {
        let t = text.trim();
        let num = |s: &str| s.trim().parse::<u64>().map_err(|_| ChopError::Invalid);
        if let Some((a, b)) = t.split_once('/') {
            return ChopStep::new(num(a)?, num(b)?);
        }
        if let Some((int, frac)) = t.split_once('.') {
            let frac = frac.trim_end_matches('0');
            if frac.len() > 9 || !frac.chars().all(|c| c.is_ascii_digit()) {
                return Err(ChopError::Invalid);
            }
            let int = if int.trim().is_empty() { 0 } else { num(int)? };
            let scale = 10u64.pow(frac.len() as u32);
            let frac = if frac.is_empty() { 0 } else { num(frac)? };
            return ChopStep::new(int * scale + frac, scale);
        }
        ChopStep::new(num(t)?, 1)
    }

    /// The length of a note value at a PPQ: a whole note is `4 × ppq` ticks.
    pub fn from_note_value(ppq: u32, value: NoteValue) -> Result<ChopStep, ChopError> {
        let (mn, md) = value.modifier.ratio();
        ChopStep::new(4 * ppq.max(1) as u64 * mn, value.denominator.max(1) as u64 * md)
    }

    pub fn is_whole(self) -> bool {
        self.denom == 1
    }

    pub fn as_f64(self) -> f64 {
        self.numer as f64 / self.denom as f64
    }

    /// Text for the input box: `120`, or `100/3` when not a whole tick.
    pub fn text(self) -> String {
        if self.is_whole() {
            self.numer.to_string()
        } else {
            format!("{}/{}", self.numer, self.denom)
        }
    }

    /// Tick of grid line `i`: `round_half_up(i × numer / denom)`.
    #[inline]
    pub fn line(self, i: u64) -> u64 {
        if self.denom == 1 {
            return i * self.numer;
        }
        // u64 arithmetic unless it would overflow (it practically never does)
        match (2 * i).checked_mul(self.numer).and_then(|x| x.checked_add(self.denom)) {
            Some(x) => x / (2 * self.denom),
            None => {
                let (n, d) = (self.numer as u128, self.denom as u128);
                ((2 * i as u128 * n + d) / (2 * d)) as u64
            }
        }
    }

    /// Index of the first grid line strictly after `tick`.
    #[inline]
    fn first_line_after(self, tick: u64) -> u64 {
        if self.denom == 1 {
            return tick / self.numer + 1;
        }
        let mut i = self.index_near(tick);
        while self.line(i) <= tick {
            i += 1;
        }
        i
    }

    /// Index of the first grid line at or after `tick`.
    #[inline]
    fn first_line_from(self, tick: u64) -> u64 {
        if self.denom == 1 {
            return tick.div_ceil(self.numer);
        }
        let mut i = self.index_near(tick);
        while self.line(i) < tick {
            i += 1;
        }
        i
    }

    /// A grid index at or just below the line nearest `tick`.
    fn index_near(self, tick: u64) -> u64 {
        let i = match tick.checked_mul(self.denom) {
            Some(x) => x / self.numer,
            None => ((tick as u128 * self.denom as u128) / self.numer as u128) as u64,
        };
        i.saturating_sub(1)
    }
}

/// Note value for computing a chop length from a PPQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteValue {
    /// 1 = whole, 2 = half, 4 = quarter … 64.
    pub denominator: u32,
    pub modifier: Modifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Plain,
    /// × 3/2
    Dotted,
    /// × 2/3
    Triplet,
}

impl Modifier {
    const fn ratio(self) -> (u64, u64) {
        match self {
            Modifier::Plain => (1, 1),
            Modifier::Dotted => (3, 2),
            Modifier::Triplet => (2, 3),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChopReport {
    /// Notes that crossed at least one grid line.
    pub notes_cut: usize,
    pub notes_before: usize,
    pub notes_after: usize,
}

/// Chop every note of every track, tracks (and big tracks' notes) in
/// parallel. Returns the chopped copy; the input is not changed.
pub fn chop_clip(clip: &Clip, step: ChopStep) -> Result<(Clip, ChopReport), ChopError> {
    // pass 1: count, so the cap is checked before anything is built and
    // every output vector is allocated once at its final size
    let counts: Vec<(usize, usize, usize)> = clip
        .nodes
        .par_iter()
        .map(|node| match node {
            TopNode::Track(t) => t
                .children
                .par_iter()
                .filter_map(|c| c.as_note())
                .map(|n| {
                    let p = pieces(n, step) as usize;
                    (1, (p > 1) as usize, p)
                })
                .reduce(|| (0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2)),
            _ => (0, 0, 0),
        })
        .collect();
    let report = counts.iter().fold(ChopReport::default(), |r, c| ChopReport {
        notes_before: r.notes_before + c.0,
        notes_cut: r.notes_cut + c.1,
        notes_after: r.notes_after + c.2,
    });
    // the only limit is memory: reserve every output track up front, so
    // running out is reported instead of aborting the program
    let need: u64 = counts.iter().map(|c| c.2 as u64).sum();
    let mut reserved: Vec<Option<Vec<TrackChild>>> = Vec::with_capacity(clip.nodes.len());
    for (node, &(before, cut, after)) in clip.nodes.iter().zip(&counts) {
        reserved.push(match node {
            TopNode::Track(t) if cut > 0 => {
                let mut v = Vec::new();
                if v.try_reserve_exact(t.children.len() - before + after).is_err() {
                    let bytes = need * std::mem::size_of::<TrackChild>() as u64;
                    return Err(ChopError::OutOfMemory(need, bytes >> 20));
                }
                Some(v)
            }
            _ => None,
        });
    }

    // pass 2: build
    let nodes: Vec<TopNode> = clip
        .nodes
        .par_iter()
        .zip(reserved)
        .map(|(node, out)| match (node, out) {
            (TopNode::Track(t), Some(out)) => TopNode::Track(chop_track(t, step, out)),
            (other, _) => other.clone(),
        })
        .collect();
    Ok((Clip { nodes, trailing: clip.trailing.clone(), warnings: clip.warnings.clone() }, report))
}

fn chop_track(track: &Track, step: ChopStep, out: Vec<TrackChild>) -> Track {
    chop_track_with(track, step, out, SEGMENT_NOTES)
}

/// Notes per parallel segment of one track.
const SEGMENT_NOTES: usize = 1 << 16;

fn chop_track_with(track: &Track, step: ChopStep, out: Vec<TrackChild>, segment_notes: usize) -> Track {
    let mut notes = track.notes();
    let in_order = match notes.next() {
        None => true,
        Some(first) => notes.try_fold(first.tick, |prev, n| (prev <= n.tick).then_some(n.tick)).is_some(),
    };
    let children = if in_order {
        // the usual case: pieces generated already in order, in parallel
        // segments of the track
        segmented(track, step, segment_notes, out).unwrap_or_else(|out| in_order_children(track, step, out))
    } else {
        let mut children = out;
        children.par_extend(track.children.par_iter().flat_map_iter(|c| Pieces::new(c, step)));
        sort_notes(&mut children);
        children
    };
    Track { children, trailing: track.trailing.clone() }
}

/// Sequential in-order chop: the pieces, poured into the note slots.
fn in_order_children(track: &Track, step: ChopStep, mut children: Vec<TrackChild>) -> Vec<TrackChild> {
    children.clear();
    let mut sorted = InOrder::new(track, step, 0, Vec::new(), 0);
    for c in &track.children {
        match c.as_note() {
            Some(n) => {
                for _ in 0..pieces(n, step) {
                    let (note, src) = sorted.next().expect("piece count");
                    children.push(track.children[src as usize].with_note(note));
                }
            }
            None => children.push(c.clone()),
        }
    }
    children
}

/// Parallel in-order chop of one track (notes in tick order).
///
/// The output is split at grid lines `T_k`. Segment `k` emits, in order,
/// every piece that starts in `[T_k, T_k+1)`: first pieces of notes starting
/// there, plus continuations of the notes still sounding at `T_k`. Sorted
/// piece number `r` goes into note slot `r` of the flattened layout (the
/// slots note `j` owns are `Q_j .. Q_j + p_j`, `Q` the prefix sum of piece
/// counts), so every segment knows exactly which output range it fills.
/// Returns `None` (caller goes sequential) when the track is small, or when
/// a note is so long that finding what sounds at a cut gets expensive.
fn segmented(track: &Track, step: ChopStep, segment_notes: usize, mut children: Vec<TrackChild>) -> Result<Vec<TrackChild>, Vec<TrackChild>> {
    let note_idx: Vec<usize> = (0..track.children.len()).into_par_iter().filter(|&i| track.children[i].is_note()).collect();
    let n = note_idx.len();
    let segments = (n / segment_notes.max(1)).min(rayon::current_num_threads() * 8);
    if segments < 2 {
        return Err(children);
    }
    let note = |j: usize| *track.children[note_idx[j]].as_note().unwrap();
    let counts: Vec<u64> = (0..n).into_par_iter().map(|j| pieces(&note(j), step)).collect();
    let mut q = Vec::with_capacity(n + 1); // Q_j, plus the total at the end
    let mut acc = 0u64;
    for &c in &counts {
        q.push(acc);
        acc += c;
    }
    q.push(acc);
    let max_gate = (0..n).into_par_iter().map(|j| note(j).gate as u64).max().unwrap_or(0);

    // cut lines (grid line indices), strictly increasing
    let mut cuts: Vec<u64> = vec![0];
    for k in 1..segments {
        let li = step.first_line_from(note(k * n / segments).tick as u64);
        if li > *cuts.last().unwrap() {
            cuts.push(li);
        }
    }

    struct Seg {
        line_index: u64,
        first_input: usize,
        active: Vec<Active>,
        rank: u64,
    }
    let limit = 4 * n / segments + 1024;
    let starts: Vec<Option<Seg>> = cuts
        .par_iter()
        .map(|&li| {
            let t = step.line(li);
            let first_input = partition(n, |j| (note(j).tick as u64) < t);
            // notes started before the cut that still sound across it
            let mut active = Vec::new();
            let mut lost = 0u64; // their pieces at or after the cut
            let mut j = first_input;
            let mut scanned = 0usize;
            while j > 0 {
                j -= 1;
                let m = note(j);
                if m.tick as u64 + max_gate <= t {
                    break;
                }
                scanned += 1;
                if scanned > limit {
                    return None;
                }
                let end = m.tick as u64 + m.gate as u64;
                if end > t {
                    active.push(Active { key: m.key, seq: note_idx[j], end, note: m, src: note_idx[j] });
                    lost += step.first_line_from(end) - li;
                }
            }
            active.sort_by_key(|a| (a.key, a.seq));
            Some(Seg { line_index: li, first_input, active, rank: q[first_input] - lost })
        })
        .collect();
    let Some(starts) = starts.into_iter().collect::<Option<Vec<Seg>>>() else { return Err(children) };

    let total = q[n];
    // where each segment's output begins: note slot r sits at output index
    // r + (non-note children before the note owning slot r)
    let owner = |r: u64| q.partition_point(|&x| x <= r) - 1;
    let len = track.children.len() - n + total as usize;
    let mut bounds: Vec<usize> = starts
        .iter()
        .enumerate()
        .map(|(k, seg)| if k == 0 { 0 } else { note_idx[owner(seg.rank)] - owner(seg.rank) + seg.rank as usize })
        .collect();
    bounds.push(len);
    if bounds.windows(2).any(|w| w[0] > w[1]) {
        return Err(children);
    }

    // every segment writes straight into its own part of the output
    children.clear();
    children.reserve_exact(len);
    let mut regions = Vec::with_capacity(starts.len());
    let mut rest = &mut children.spare_capacity_mut()[..len];
    for w in bounds.windows(2) {
        let (head, tail) = rest.split_at_mut(w[1] - w[0]);
        regions.push(head);
        rest = tail;
    }
    let ok = regions.into_par_iter().enumerate().all(|(k, region)| {
        let seg = &starts[k];
        let (r0, r1) = (seg.rank, starts.get(k + 1).map_or(total, |s| s.rank));
        let stop = starts.get(k + 1).map_or(u64::MAX, |s| step.line(s.line_index));
        let mut sorted = InOrder::new(track, step, seg.first_input, seg.active.clone(), seg.line_index);
        // note slot r0 is `skip` slots into note j's block
        let mut j = if k == 0 { 0 } else { owner(r0) };
        let mut skip = r0 - q[j];
        let mut child = if k == 0 { 0 } else { note_idx[j] };
        let mut w = 0usize;
        let mut put = |c: TrackChild| match region.get_mut(w) {
            Some(slot) => {
                slot.write(c);
                w += 1;
                true
            }
            None => false,
        };
        let mut r = r0;
        while child < track.children.len() {
            let c = &track.children[child];
            if c.is_note() {
                if r == r1 && k + 1 < starts.len() {
                    break;
                }
                let block = q[j + 1] - q[j];
                while skip < block && r < r1 {
                    let Some((piece, src)) = sorted.next() else { return false };
                    if piece.tick as u64 >= stop || !put(track.children[src].with_note(piece)) {
                        return false;
                    }
                    r += 1;
                    skip += 1;
                }
                if skip < block {
                    break; // the next segment continues this block
                }
                j += 1;
                skip = 0;
            } else if !put(c.clone()) {
                return false;
            }
            child += 1;
        }
        // all of this segment's pieces, and nothing of the next one's
        let next_ok = !matches!(sorted.next(), Some((piece, _)) if (piece.tick as u64) < stop);
        next_ok && r == r1 && w == region.len()
    });
    if !ok {
        // cannot happen for well-formed input; anything written is leaked,
        // never read, and the caller recomputes sequentially
        return Err(Vec::new());
    }
    // SAFETY: every one of the `len` slots was written exactly once above
    // (each region was checked to be filled completely).
    unsafe { children.set_len(len) };
    Ok(children)
}

/// First index in `0..n` where `pred` is false (pred true then false).
fn partition(n: usize, pred: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if pred(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// A note still sounding across the next grid line. `seq` orders notes
/// with the same key: the one that started earlier comes first.
#[derive(Clone, Copy)]
struct Active {
    key: u8,
    seq: usize,
    end: u64,
    note: NoteItem,
    src: usize,
}

/// Notes in (tick, key) order from notes in tick order: only the notes that
/// share a tick are sorted (stably, by key), a few at a time.
struct ByTickKey<'a, I: Iterator<Item = (usize, NoteItem)>> {
    inner: std::iter::Peekable<I>,
    group: Vec<(usize, NoteItem)>,
    pos: usize,
    _track: std::marker::PhantomData<&'a ()>,
}

impl<'a, I: Iterator<Item = (usize, NoteItem)>> ByTickKey<'a, I> {
    fn new(inner: I) -> Self {
        ByTickKey { inner: inner.peekable(), group: Vec::new(), pos: 0, _track: std::marker::PhantomData }
    }

    #[inline]
    fn peek(&mut self) -> Option<&(usize, NoteItem)> {
        if self.pos == self.group.len() {
            self.refill();
        }
        self.group.get(self.pos)
    }

    #[inline]
    fn next(&mut self) -> Option<(usize, NoteItem)> {
        let item = self.peek().copied();
        self.pos += item.is_some() as usize;
        item
    }

    fn refill(&mut self) {
        self.group.clear();
        self.pos = 0;
        let Some(first) = self.inner.next() else { return };
        self.group.push(first);
        while let Some(&next) = self.inner.peek() {
            if next.1.tick != first.1.tick {
                break;
            }
            self.group.push(next);
            self.inner.next();
        }
        if !self.group.windows(2).all(|w| w[0].1.key <= w[1].1.key) {
            self.group.sort_by_key(|g| g.1.key);
        }
    }
}

/// All pieces of a track whose notes are in tick order, produced lazily in
/// (tick, key) order: exactly the order a stable sort of the pieces would
/// give, without sorting or buffering. Each piece carries the index of its
/// source child.
///
/// Every piece after a note's first starts on a grid line, so the output is
/// a merge of the input with the notes still sounding (`active`, kept in
/// (key, input order)) at each successive grid line.
struct InOrder<'a, I: Iterator<Item = (usize, NoteItem)>> {
    input: ByTickKey<'a, I>,
    step: ChopStep,
    active: Vec<Active>,
    next_active: Vec<Active>,
    /// Next entry of `active` to continue at the current line.
    a: usize,
    li: u64,
    line: u64,
    after: u64,
    phase: Phase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Pick the next grid line.
    Start,
    /// First pieces of notes starting before `line`.
    Before,
    /// Pieces starting on `line`.
    AtLine,
    Done,
}

type NoteIter<'a> = std::iter::FilterMap<std::iter::Enumerate<std::slice::Iter<'a, TrackChild>>, fn((usize, &TrackChild)) -> Option<(usize, NoteItem)>>;

impl<'a> InOrder<'a, std::iter::Skip<NoteIter<'a>>> {
    /// Start at note number `first_input`, with `active` sounding across grid
    /// line `line_index` (empty and 0 to start at the beginning).
    fn new(track: &'a Track, step: ChopStep, first_input: usize, active: Vec<Active>, line_index: u64) -> Self {
        fn note_of((i, c): (usize, &TrackChild)) -> Option<(usize, NoteItem)> {
            c.as_note().map(|n| (i, *n))
        }
        let notes: NoteIter<'a> = track.children.iter().enumerate().filter_map(note_of as fn((usize, &TrackChild)) -> Option<(usize, NoteItem)>);
        InOrder {
            input: ByTickKey::new(notes.skip(first_input)),
            step,
            active,
            next_active: Vec::new(),
            a: 0,
            li: line_index,
            line: 0,
            after: 0,
            phase: Phase::Start,
        }
    }
}

impl<I: Iterator<Item = (usize, NoteItem)>> Iterator for InOrder<'_, I> {
    type Item = (NoteItem, usize);

    #[inline]
    fn next(&mut self) -> Option<(NoteItem, usize)> {
        loop {
            match self.phase {
                Phase::Done => return None,
                Phase::Start => {
                    if self.active.is_empty() {
                        match self.input.peek() {
                            None => {
                                self.phase = Phase::Done;
                                return None;
                            }
                            Some(&(_, n)) => self.li = self.step.first_line_after(n.tick as u64),
                        }
                    }
                    self.line = self.step.line(self.li);
                    self.phase = Phase::Before;
                }
                Phase::Before => {
                    match self.input.peek() {
                        Some(&(src, n)) if (n.tick as u64) < self.line => {
                            self.input.next();
                            let end = n.tick as u64 + n.gate as u64;
                            if end > self.line {
                                let a = Active { key: n.key, seq: src, end, note: n, src };
                                let at = self.active.partition_point(|x| (x.key, x.seq) < (a.key, a.seq));
                                self.active.insert(at, a);
                            }
                            return Some((NoteItem { gate: (end.min(self.line) - n.tick as u64) as u32, ..n }, src));
                        }
                        _ => {
                            self.after = self.step.line(self.li + 1);
                            self.a = 0;
                            self.phase = Phase::AtLine;
                        }
                    }
                }
                Phase::AtLine => {
                    // continuing notes merged with notes starting on the line
                    // (same key: the continuation comes first, it started earlier)
                    let line = self.line;
                    let here = self.input.peek().filter(|(_, n)| n.tick as u64 == line).map(|(_, n)| n.key);
                    let take_input = match self.active.get(self.a) {
                        Some(x) => here.is_some_and(|k| k < x.key),
                        None if here.is_some() => true,
                        None => {
                            std::mem::swap(&mut self.active, &mut self.next_active);
                            self.next_active.clear();
                            self.li += 1;
                            self.phase = Phase::Start;
                            continue;
                        }
                    };
                    let x = if take_input {
                        let (src, n) = self.input.next().unwrap();
                        Active { key: n.key, seq: src, end: n.tick as u64 + n.gate as u64, note: n, src }
                    } else {
                        self.a += 1;
                        self.active[self.a - 1]
                    };
                    if x.end > self.after {
                        self.next_active.push(x);
                    }
                    return Some((NoteItem { tick: line as u32, gate: (x.end.min(self.after) - line) as u32, ..x.note }, x.src));
                }
            }
        }
    }
}

/// How many notes a note becomes (1 = it crosses no grid line).
fn pieces(note: &NoteItem, step: ChopStep) -> u64 {
    let start = note.tick as u64;
    let end = start + note.gate as u64;
    let first = step.first_line_after(start);
    if step.line(first) >= end {
        return 1;
    }
    step.first_line_from(end) - first + 1
}

/// The pieces of one child (just the child itself unless it is a note).
struct Pieces<'a> {
    child: Option<&'a TrackChild>,
    note: NoteItem,
    end: u64,
    line: u64,
    step: ChopStep,
}

impl<'a> Pieces<'a> {
    fn new(child: &'a TrackChild, step: ChopStep) -> Pieces<'a> {
        let note = child.as_note().copied().unwrap_or(NoteItem::canonical(0, 0, 0, 0));
        Pieces {
            child: Some(child),
            note,
            end: note.tick as u64 + note.gate as u64,
            line: step.first_line_after(note.tick as u64),
            step,
        }
    }
}

impl Iterator for Pieces<'_> {
    type Item = TrackChild;

    fn next(&mut self) -> Option<TrackChild> {
        let child = self.child?;
        if !child.is_note() {
            self.child = None;
            return Some(child.clone());
        }
        let start = self.note.tick as u64;
        let cut = self.step.line(self.line).min(self.end);
        let piece = child.with_note(NoteItem { gate: (cut - start) as u32, ..self.note });
        if cut >= self.end {
            self.child = None;
        } else {
            self.note.tick = cut as u32;
            self.line += 1;
        }
        Some(piece)
    }
}

/// Stable-sort the notes by (tick, key) while every non-note child keeps its
/// slot (only for tracks whose notes weren't in order to begin with).
fn sort_notes(children: &mut [TrackChild]) {
    let key = |c: &TrackChild| c.as_note().map(|n| (n.tick, n.key)).unwrap_or_default();
    let slots: Vec<usize> = (0..children.len()).filter(|&i| children[i].is_note()).collect();
    let (Some(&first), Some(&last)) = (slots.first(), slots.last()) else { return };
    if last - first + 1 == slots.len() {
        children[first..=last].par_sort_by_key(key);
        return;
    }
    let mut notes: Vec<TrackChild> =
        slots.iter().map(|&i| std::mem::replace(&mut children[i], TrackChild::length(0))).collect();
    notes.par_sort_by_key(key);
    for (slot, note) in slots.into_iter().zip(notes) {
        children[slot] = note;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::clip::{decode_clip, encode_clip, self_check, EventItem};
    use crate::format::events;
    use crate::format::template::tests::golden_unpacked;

    fn notes(clip: &Clip) -> Vec<(usize, u32, u32, u8)> {
        clip.note_views().iter().map(|n| (n.track, n.tick, n.gate, n.key)).collect()
    }

    #[test]
    fn chop_every_n_ticks_in_every_track() {
        // golden: (0,240,36) (240,240,38) (480,480,127)
        let mut clip = decode_clip(&golden_unpacked());
        let second = clip.tracks().next().unwrap().clone();
        clip.nodes.push(TopNode::Track(second));
        let (out, report) = chop_clip(&clip, ChopStep::ticks(120).unwrap()).unwrap();
        assert_eq!(report.notes_cut, 6);
        assert_eq!(out.note_count(), 2 * 8);
        let t0: Vec<_> = notes(&out).into_iter().filter(|n| n.0 == 0).map(|n| (n.1, n.2, n.3)).collect();
        assert_eq!(
            t0,
            vec![
                (0, 120, 36),
                (120, 120, 36),
                (240, 120, 38),
                (360, 120, 38),
                (480, 120, 127),
                (600, 120, 127),
                (720, 120, 127),
                (840, 120, 127),
            ]
        );
        assert_eq!(out.stretch_len(), Some(960));
        self_check(&out).unwrap();
    }

    #[test]
    fn grid_starts_at_clip_start_not_note_start() {
        let mut track = Track::default();
        track.children.push(TrackChild::Note(NoteItem::canonical(100, 300, 60, 90)));
        let mut clip = Clip::default();
        clip.push(TopNode::Track(track));
        let (out, _) = chop_clip(&clip, ChopStep::ticks(240).unwrap()).unwrap();
        assert_eq!(notes(&out), vec![(0, 100, 140, 60), (0, 240, 160, 60)]);
        let vel: Vec<_> = out.note_views().iter().map(|n| n.vel).collect();
        assert_eq!(vel, vec![90, 90]);
    }

    #[test]
    fn nothing_to_cut_is_byte_identical() {
        let clip = decode_clip(&golden_unpacked());
        let (out, report) = chop_clip(&clip, ChopStep::ticks(240).unwrap()).unwrap();
        // only the 480-gate note crosses 720
        assert_eq!(report.notes_cut, 1);
        let (out2, report2) = chop_clip(&out, ChopStep::ticks(240).unwrap()).unwrap();
        assert_eq!(report2.notes_cut, 0);
        assert_eq!(encode_clip(&out2), encode_clip(&out));
        let (same, _) = chop_clip(&clip, ChopStep::ticks(960).unwrap()).unwrap();
        assert_eq!(encode_clip(&same), golden_unpacked());
    }

    #[test]
    fn events_keep_their_slot_and_bytes() {
        let mut track = Track::default();
        track.children.push(TrackChild::Note(NoteItem::canonical(0, 480, 60, 100)));
        let ev: EventItem = events::control(100, 7, 100.0);
        track.children.push(TrackChild::event(ev.clone()));
        track.children.push(TrackChild::Note(NoteItem::canonical(200, 10, 50, 100)));
        let mut clip = Clip::default();
        clip.push(TopNode::Track(track));
        let (out, _) = chop_clip(&clip, ChopStep::ticks(240).unwrap()).unwrap();
        let t = out.tracks().next().unwrap();
        // the first note became two pieces; the event follows them unchanged
        assert!(matches!(&t.children[2], TrackChild::Event(e) if **e == ev));
        let ticks: Vec<u32> = t.notes().map(|n| n.tick).collect();
        assert_eq!(ticks, vec![0, 200, 240]);
    }

    /// The in-order merge must give exactly what chopping every note and then
    /// stable-sorting the pieces gives.
    #[test]
    fn in_order_merge_equals_sorting() {
        let mut rng = 12345u32;
        let mut next = move |m: u32| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng % m
        };
        for round in 0..200 {
            let mut track = Track::default();
            let mut tick = 0u32;
            for _ in 0..(1 + next(60)) {
                tick += next(4) * next(50);
                track.children.push(TrackChild::Note(NoteItem::canonical(tick, next(700), next(6) as u8 * 3, next(127) as u8)));
                if next(9) == 0 {
                    track.children.push(TrackChild::event(events::control(tick, 7, 1.0)));
                }
            }
            // input in tick order (same-tick keys in any order)
            let step = match round % 3 {
                0 => ChopStep::ticks(1 + next(200)).unwrap(),
                1 => ChopStep::new(100 + next(300) as u64, 3).unwrap(),
                _ => ChopStep::ticks(1).unwrap(),
            };
            let fast = chop_track_with(&track, step, Vec::new(), usize::MAX);
            let mut slow: Vec<TrackChild> = track.children.iter().flat_map(|c| Pieces::new(c, step)).collect();
            sort_notes(&mut slow);
            assert_eq!(fast.children, slow, "round {round}");
        }
    }

    #[test]
    fn parallel_segments_equal_sequential() {
        let mut rng = 999u32;
        let mut next = move |m: u32| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng % m
        };
        for round in 0..60 {
            let mut track = Track::default();
            track.children.push(TrackChild::raw(crate::format::chunk::Item { tag: 1000, body: vec![0, 0] }));
            let mut tick = 0u32;
            for _ in 0..(200 + next(3000)) {
                tick += next(3) * next(40);
                let gate = if next(50) == 0 { next(5000) } else { next(400) };
                track.children.push(TrackChild::Note(NoteItem::canonical(tick, gate, next(5) as u8 * 7, 1 + next(126) as u8)));
                if next(15) == 0 {
                    track.children.push(TrackChild::event(events::control(tick, 7, 1.0)));
                }
            }
            track.children.push(TrackChild::length(tick + 10));
            let step = match round % 3 {
                0 => ChopStep::ticks(1 + next(150)).unwrap(),
                1 => ChopStep::new(50 + next(300) as u64, 7).unwrap(),
                _ => ChopStep::ticks(13).unwrap(),
            };
            let sequential = in_order_children(&track, step, Vec::new());
            let parallel = segmented(&track, step, 1 + next(40) as usize, Vec::new()).ok().expect("segmented");
            assert_eq!(parallel, sequential, "round {round}");
        }
    }

    #[test]
    fn note_values_from_ppq() {
        let at = |ppq, denominator, modifier| {
            ChopStep::from_note_value(ppq, NoteValue { denominator, modifier }).unwrap().text()
        };
        assert_eq!(at(480, 4, Modifier::Plain), "480");
        assert_eq!(at(480, 16, Modifier::Plain), "120");
        assert_eq!(at(480, 8, Modifier::Triplet), "160");
        assert_eq!(at(480, 4, Modifier::Dotted), "720");
        assert_eq!(at(960, 1, Modifier::Plain), "3840");
        assert_eq!(at(100, 8, Modifier::Triplet), "100/3");
        assert_eq!(
            ChopStep::from_note_value(6, NoteValue { denominator: 64, modifier: Modifier::Plain }),
            Err(ChopError::TooShort)
        );
    }

    #[test]
    fn fractional_steps_do_not_drift() {
        let step = ChopStep::parse("100/3").unwrap();
        assert_eq!((step.line(1), step.line(2), step.line(3)), (33, 67, 100));
        assert_eq!(step.line(30_000), 1_000_000);
        let mut track = Track::default();
        track.children.push(TrackChild::Note(NoteItem::canonical(0, 100, 60, 100)));
        let mut clip = Clip::default();
        clip.push(TopNode::Track(track));
        let (out, _) = chop_clip(&clip, step).unwrap();
        let gates: Vec<u32> = out.note_views().iter().map(|n| n.gate).collect();
        assert_eq!(gates, vec![33, 34, 33]);
    }

    #[test]
    fn parses_inputs() {
        assert_eq!(ChopStep::parse(" 120 ").unwrap().text(), "120");
        assert_eq!(ChopStep::parse("320/2").unwrap().text(), "160");
        assert_eq!(ChopStep::parse("33.5").unwrap().text(), "67/2");
        assert_eq!(ChopStep::parse("120.0").unwrap().text(), "120");
        assert_eq!(ChopStep::parse("0"), Err(ChopError::Invalid));
        assert_eq!(ChopStep::parse("1/2"), Err(ChopError::TooShort));
        assert_eq!(ChopStep::parse("abc"), Err(ChopError::Invalid));
        assert_eq!(ChopStep::parse("4/0"), Err(ChopError::Invalid));
    }
}
