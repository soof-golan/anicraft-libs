//! Small data-parallel helpers. Every unit of work is independent (a pixel or a row), so the result never
//! depends on how rayon schedules it.

use rayon::prelude::*;

/// Buffers shorter than this many bytes are processed on the calling thread.
const PARALLEL_MIN_BYTES: usize = 256 * 1024;
/// Bytes handed to one rayon task (a multiple of 4 so tasks start on pixel boundaries).
const TASK_BYTES: usize = 64 * 1024;

/// Replaces every RGBA pixel of `data` with `f(pixel)`. `f` must depend on the pixel alone: a pixel equal to the one
/// before it reuses that result, so flat areas cost one call per run instead of one per pixel.
pub fn for_each_pixel(data: &mut [u8], f: impl Fn(&mut [u8; 4]) + Sync + Send) {
    let run = |bytes: &mut [u8]| {
        let mut last: Option<([u8; 4], [u8; 4])> = None;
        for px in bytes.as_chunks_mut::<4>().0 {
            match last {
                Some((input, output)) if *px == input => *px = output,
                _ => {
                    let input = *px;
                    f(px);
                    last = Some((input, *px));
                }
            }
        }
    };
    if data.len() < PARALLEL_MIN_BYTES {
        run(data);
    } else {
        data.par_chunks_mut(TASK_BYTES).for_each(run);
    }
}

/// Calls `f(scratch, row_index, row)` for every row of `row_len` elements, in parallel when the buffer is large.
/// `init` makes one scratch value per worker, so rows can reuse allocations; `f` must not let one row's result
/// depend on what an earlier row left in the scratch.
pub fn for_each_row_with<T: Send, S: Send>(
    data: &mut [T],
    row_len: usize,
    init: impl Fn() -> S + Sync + Send,
    f: impl Fn(&mut S, usize, &mut [T]) + Sync + Send,
) {
    if row_len == 0 {
        return;
    }
    if data.len().saturating_mul(size_of::<T>()) < PARALLEL_MIN_BYTES {
        let mut scratch = init();
        data.chunks_mut(row_len).enumerate().for_each(|(row, values)| f(&mut scratch, row, values));
    } else {
        data.par_chunks_mut(row_len).enumerate().for_each_init(init, |scratch, (row, values)| f(scratch, row, values));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_visits_cover_every_pixel_once() {
        let mut big = vec![0_u8; 1 << 20];
        for_each_pixel(&mut big, |px| px.iter_mut().for_each(|b| *b = b.wrapping_add(1)));
        assert!(big.iter().all(|&b| b == 1));
        let mut small = vec![0_u8; 40];
        for_each_pixel(&mut small, |px| px.iter_mut().for_each(|b| *b = b.wrapping_add(1)));
        assert!(small.iter().all(|&b| b == 1));
    }

    #[test]
    fn rows_get_their_index() {
        let mut v = vec![0_usize; 6];
        for_each_row_with(&mut v, 3, || (), |(), i, row| row.iter_mut().for_each(|x| *x = i));
        assert_eq!(v, vec![0, 0, 0, 1, 1, 1]);
        let mut big = vec![0_u32; 300_000];
        for_each_row_with(&mut big, 1000, Vec::<u32>::new, |scratch, i, row| {
            scratch.clear();
            scratch.push(u32::try_from(i).unwrap());
            for x in row.iter_mut() {
                *x = scratch[0];
            }
        });
        assert!(big.chunks(1000).enumerate().all(|(i, row)| row.iter().all(|&x| x == u32::try_from(i).unwrap())));
    }
}
