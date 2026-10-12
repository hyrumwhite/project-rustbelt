//! The Windows C runtime's `qsort` (the UCRT's), as the game's Windows build
//! sorts: a quicksort on a median of three that sorts runs of eight or fewer
//! by selection, keeping its explicit stack. It isn't stable, so elements
//! that compare equal end in its order; a port that sorts what the game
//! sorts takes this one to agree with it.

use std::cmp::Ordering;

/// Runs this short are sorted by selection.
const CUTOFF: usize = 8;

/// The selection sort of `lo..=hi`: the greatest moved to the end, again
/// and again.
fn shortsort<T>(items: &mut [T], lo: usize, mut hi: usize, compare: &mut impl FnMut(&T, &T) -> Ordering) {
    while hi > lo {
        let mut max = lo;
        for p in lo + 1..=hi {
            if compare(&items[p], &items[max]) == Ordering::Greater {
                max = p;
            }
        }
        items.swap(max, hi);
        hi -= 1;
    }
}

/// `qsort(items, count, sizeof *items, compare)`.
pub fn qsort<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
    let num = items.len();
    if num < 2 {
        return;
    }
    let gt = Ordering::Greater;
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut lo = 0usize;
    let mut hi = num - 1;
    loop {
        let size = hi - lo + 1;
        if size <= CUTOFF {
            shortsort(items, lo, hi, &mut compare);
        } else {
            let mut mid = lo + size / 2;
            if compare(&items[lo], &items[mid]) == gt {
                items.swap(lo, mid);
            }
            if compare(&items[lo], &items[hi]) == gt {
                items.swap(lo, hi);
            }
            if compare(&items[mid], &items[hi]) == gt {
                items.swap(mid, hi);
            }
            // indices may step one below lo (as the C's pointers do) only
            // where the loops stop, so they're kept signed
            let mut loguy = lo as isize;
            let mut higuy = hi as isize;
            let (lo_i, hi_i) = (lo as isize, hi as isize);
            loop {
                if (mid as isize) > loguy {
                    loop {
                        loguy += 1;
                        if !(loguy < mid as isize && compare(&items[loguy as usize], &items[mid]) != gt) {
                            break;
                        }
                    }
                }
                if (mid as isize) <= loguy {
                    loop {
                        loguy += 1;
                        if !(loguy <= hi_i && compare(&items[loguy as usize], &items[mid]) != gt) {
                            break;
                        }
                    }
                }
                loop {
                    higuy -= 1;
                    if !(higuy > mid as isize && compare(&items[higuy as usize], &items[mid]) == gt) {
                        break;
                    }
                }
                if higuy < loguy {
                    break;
                }
                items.swap(loguy as usize, higuy as usize);
                if mid as isize == higuy {
                    mid = loguy as usize;
                }
            }
            higuy += 1;
            if (mid as isize) < higuy {
                loop {
                    higuy -= 1;
                    if !(higuy > mid as isize && compare(&items[higuy as usize], &items[mid]) == Ordering::Equal) {
                        break;
                    }
                }
            }
            if (mid as isize) >= higuy {
                loop {
                    higuy -= 1;
                    if !(higuy > lo_i && compare(&items[higuy as usize], &items[mid]) == Ordering::Equal) {
                        break;
                    }
                }
            }
            if higuy - lo_i >= hi_i - loguy {
                if lo_i < higuy {
                    stack.push((lo, higuy as usize));
                }
                if loguy < hi_i {
                    lo = loguy as usize;
                    continue;
                }
            } else {
                if loguy < hi_i {
                    stack.push((loguy as usize, hi));
                }
                if lo_i < higuy {
                    hi = higuy as usize;
                    continue;
                }
            }
        }
        match stack.pop() {
            Some((next_lo, next_hi)) => {
                lo = next_lo;
                hi = next_hi;
            }
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hash of the UCRT's `qsort` order (32-bit Windows, clang) over
    /// 200,000 arrays of up to 128 entries of few distinct keys, from the
    /// same generator.
    #[test]
    fn agrees_with_the_ucrt() {
        let mut seed: u32 = 12345;
        let mut next = || {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            (seed >> 8) & 0xFFFFFF
        };
        let mut hash: u32 = 2166136261;
        for _ in 0..200000 {
            let n = (next() % 129) as usize;
            let keys = next() % 20 + 1;
            let mut entries: Vec<(u32, f32)> = (0..n).map(|i| (i as u32, 0.0)).collect();
            for entry in entries.iter_mut() {
                entry.1 = (next() % keys) as f32;
            }
            qsort(&mut entries, |a, b| {
                if a.1 < b.1 {
                    Ordering::Less
                } else if a.1 > b.1 {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            });
            for entry in &entries {
                hash = (hash ^ entry.0).wrapping_mul(16777619);
            }
        }
        assert_eq!(hash, 1611981029);
    }
}
