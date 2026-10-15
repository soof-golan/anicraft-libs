//! Offscreen layers and the buffer pool that lets a [`crate::Renderer`] reuse them from frame to frame.

use anim_core::num::u32_to_usize;
use tiny_skia::{IntSize, Pixmap};

use crate::px::PxRect;

/// A premultiplied RGBA8 pixmap that covers `rect` in device pixels: pixel (0, 0) of the pixmap is device pixel
/// `(rect.x0, rect.y0)`.
#[derive(Debug)]
pub struct Layer {
    pub pm: Pixmap,
    pub rect: PxRect,
}

impl Layer {
    /// Device pixels to layer pixels.
    pub const fn origin(&self) -> (i32, i32) {
        (self.rect.x0, self.rect.y0)
    }
}

/// Free buffers, smallest sufficient first, and a budget on live bytes.
#[derive(Debug, Default)]
pub struct Pool {
    free: Vec<Vec<u8>>,
    live_bytes: usize,
    peak_bytes: usize,
}

/// Buffers kept for reuse; more than this are dropped.
const MAX_FREE_BUFFERS: usize = 8;
/// Total bytes of free buffers kept for reuse.
const MAX_FREE_BYTES: usize = 512 * 1024 * 1024;
/// Upper bound on bytes held by live layers; groups that would exceed it are drawn without isolation.
pub const MAX_LIVE_BYTES: usize = 1 << 30;

impl Pool {
    /// A zeroed layer for `rect`, or `None` when the rectangle is empty, too large or would break the budget.
    pub fn take(&mut self, rect: PxRect) -> Option<Layer> {
        if rect.is_empty() {
            return None;
        }
        let (w, h) = (rect.width(), rect.height());
        let bytes = u32_to_usize(w).checked_mul(u32_to_usize(h))?.checked_mul(4)?;
        if self.live_bytes.checked_add(bytes)? > MAX_LIVE_BYTES {
            return None;
        }
        let best = self.free.iter().enumerate().filter(|(_, b)| b.capacity() >= bytes).min_by_key(|(_, b)| b.capacity()).map(|(i, _)| i);
        let mut buf = best.map_or_else(Vec::new, |i| self.free.swap_remove(i));
        buf.clear();
        buf.try_reserve_exact(bytes).ok()?;
        buf.resize(bytes, 0);
        let pm = Pixmap::from_vec(buf, IntSize::from_wh(w, h)?)?;
        self.live_bytes = self.live_bytes.saturating_add(bytes);
        self.peak_bytes = self.peak_bytes.max(self.live_bytes);
        Some(Layer { pm, rect })
    }

    /// Returns a layer's buffer to the pool.
    pub fn recycle(&mut self, layer: Layer) {
        let buf = layer.pm.take();
        self.live_bytes = self.live_bytes.saturating_sub(buf.len());
        self.keep(buf);
    }

    /// Takes a layer's buffer out of the budget without keeping it (the caller owns the bytes now).
    pub fn release(&mut self, layer: Layer) -> Vec<u8> {
        let buf = layer.pm.take();
        self.live_bytes = self.live_bytes.saturating_sub(buf.len());
        buf
    }

    /// Offers a spare buffer (for example a finished frame the caller no longer needs) to the pool.
    pub fn keep(&mut self, buf: Vec<u8>) {
        if self.free.len() >= MAX_FREE_BUFFERS {
            return;
        }
        let kept: usize = self.free.iter().map(Vec::capacity).sum();
        if kept.saturating_add(buf.capacity()) <= MAX_FREE_BYTES {
            self.free.push(buf);
        }
    }

    pub const fn peak_bytes(&self) -> usize {
        self.peak_bytes
    }

    pub const fn reset_peak(&mut self) {
        self.peak_bytes = self.live_bytes;
    }

    pub const fn live_bytes(&self) -> usize {
        self.live_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_are_zeroed_and_buffers_reused() {
        let mut pool = Pool::default();
        let mut a = pool.take(PxRect::new(0, 0, 4, 4)).unwrap();
        a.pm.data_mut().fill(9);
        let ptr = a.pm.data().as_ptr();
        pool.recycle(a);
        assert_eq!(pool.live_bytes(), 0);
        let b = pool.take(PxRect::new(0, 0, 2, 2)).unwrap();
        assert_eq!(b.pm.data().as_ptr(), ptr);
        assert!(b.pm.data().iter().all(|&v| v == 0));
        assert_eq!(b.pm.data().len(), 16);
    }

    #[test]
    fn empty_and_oversized_layers_are_refused() {
        let mut pool = Pool::default();
        assert!(pool.take(PxRect::new(0, 0, 0, 5)).is_none());
        assert!(pool.take(PxRect::new(0, 0, 40_000, 40_000)).is_none());
    }
}
